/*
 * Morpheus patch: hardware H.264 decoding on Windows through Media
 * Foundation. See mf_h264_decoder.h.
 *
 * The decoder is a webrtc::VideoDecoder around Microsoft's H.264 decoder
 * MFT (synchronous). Given an IMFDXGIDeviceManager with a Direct3D 11 video
 * device (MFT_MESSAGE_SET_D3D_MANAGER) it decodes through DXVA on the GPU
 * and hands out NV12 textures, copied here through a staging texture into
 * I420 buffers. Low-latency mode, so each picture comes out as soon as it
 * went in. Without a device (the CI self-test) it decodes in software into
 * memory buffers.
 *
 * The factory wraps it in libwebrtc's software fallback: if the MFT does
 * not open on the GPU, fails, gives no pictures, or decodes in software
 * after all, FFmpeg takes over.
 */

#include "mf_h264_decoder.h"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <optional>

#include "absl/strings/match.h"
#include "api/video/encoded_image.h"
#include "api/video/i420_buffer.h"
#include "api/video/video_frame.h"
#include "api/video_codecs/h264_profile_level_id.h"
#include "api/video_codecs/video_decoder_software_fallback_wrapper.h"
#include "modules/video_coding/codecs/h264/include/h264.h"
#include "modules/video_coding/include/video_error_codes.h"
#include "rtc_base/logging.h"
#include "third_party/libyuv/include/libyuv/convert.h"

#include "mf_h264_encoder.h"

#include <windows.h>
// The GUIDs this file uses (Media Foundation's, D3D11's decoder profiles),
// defined here (selectany), so nothing links mfuuid.lib.
#include <initguid.h>
#include <strmif.h>
#include <codecapi.h>
#include <d3d11.h>
#include <d3d11_4.h>
#include <dxgi.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <mftransform.h>
#include <wrl/client.h>

#include "mf_common.h"

using Microsoft::WRL::ComPtr;

namespace livekit_ffi {

namespace {

using mf::EnsureCom;
using mf::FriendlyName;
using mf::Hex;
using mf::Mf;
using mf::MfApi;
using mf::MfStarted;
using mf::Utf8;

constexpr char kImplementationName[] = "Media Foundation H264 Decoder";
// Sample times given to the MFT (100 ns units), one picture apart.
constexpr LONGLONG kPictureTime = 333333;
// Pictures in a row that gave nothing out before FFmpeg takes over (a
// low-latency decoder gives each one out at once).
constexpr int kMaxSilent = 30;

// The default GPU's Direct3D 11 device for video, shared with the MFT
// through a DXGI device manager.
struct Gpu {
  ComPtr<ID3D11Device> device;
  ComPtr<ID3D11DeviceContext> context;
  ComPtr<IMFDXGIDeviceManager> manager;
  std::string adapter;
  bool software_adapter = false;
};

bool CreateGpu(Gpu* gpu, std::string* why) {
  const MfApi* mf = Mf();
  if (!mf || !mf->create_dxgi_device_manager) {
    *why = "no MFCreateDXGIDeviceManager";
    return false;
  }
  static const D3D_FEATURE_LEVEL kLevels[] = {
      D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1,
      D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_9_3};
  const UINT flags =
      D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT;
  HRESULT hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
                                 flags, kLevels, ARRAYSIZE(kLevels),
                                 D3D11_SDK_VERSION, &gpu->device, nullptr,
                                 &gpu->context);
  if (hr == E_INVALIDARG) {
    // Direct3D 11.0 runtimes do not know 11_1.
    hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags,
                           kLevels + 1, ARRAYSIZE(kLevels) - 1,
                           D3D11_SDK_VERSION, &gpu->device, nullptr,
                           &gpu->context);
  }
  if (FAILED(hr)) {
    *why = "no Direct3D 11 video device (" + Hex(hr) + ")";
    return false;
  }
  ComPtr<IDXGIDevice> dxgi;
  ComPtr<IDXGIAdapter> adapter;
  DXGI_ADAPTER_DESC desc = {};
  if (SUCCEEDED(gpu->device.As(&dxgi)) &&
      SUCCEEDED(dxgi->GetAdapter(&adapter)) &&
      SUCCEEDED(adapter->GetDesc(&desc))) {
    gpu->adapter = Utf8(desc.Description);
    // Microsoft Basic Render Driver (no GPU, e.g. CI machines).
    gpu->software_adapter = desc.VendorId == 0x1414;
  }
  // The MFT uses the device from its own threads too.
  ComPtr<ID3D11Multithread> multithread;
  if (SUCCEEDED(gpu->device.As(&multithread)))
    multithread->SetMultithreadProtected(TRUE);
  UINT token = 0;
  hr = mf->create_dxgi_device_manager(&token, &gpu->manager);
  if (SUCCEEDED(hr))
    hr = gpu->manager->ResetDevice(gpu->device.Get(), token);
  if (FAILED(hr)) {
    *why = "DXGI device manager " + Hex(hr);
    return false;
  }
  return true;
}

// Whether the device decodes H.264 (DXVA's VLD profile) into NV12.
bool DecodesH264(ID3D11Device* device) {
  ComPtr<ID3D11VideoDevice> video;
  if (FAILED(device->QueryInterface(IID_PPV_ARGS(&video))))
    return false;
  UINT count = video->GetVideoDecoderProfileCount();
  for (UINT i = 0; i < count; ++i) {
    GUID profile = {};
    if (FAILED(video->GetVideoDecoderProfile(i, &profile)))
      continue;
    if (profile != D3D11_DECODER_PROFILE_H264_VLD_NOFGT)
      continue;
    BOOL supported = FALSE;
    if (SUCCEEDED(video->CheckVideoDecoderFormat(&profile, DXGI_FORMAT_NV12,
                                                 &supported)) &&
        supported)
      return true;
  }
  return false;
}

void SetBool(ICodecAPI* api, const GUID& what, bool value) {
  if (!api)
    return;
  VARIANT v;
  memset(&v, 0, sizeof(v));
  v.vt = VT_BOOL;
  v.boolVal = value ? VARIANT_TRUE : VARIANT_FALSE;
  api->SetValue(&what, &v);
}

class MediaFoundationH264Decoder : public webrtc::VideoDecoder {
 public:
  // `gpu`: decode through Direct3D 11 (fails rather than decode in
  // software); otherwise Microsoft's decoder in software (self-test).
  explicit MediaFoundationH264Decoder(bool gpu) : use_gpu_(gpu) {}

  ~MediaFoundationH264Decoder() override { Close(); }

  bool Configure(const Settings& settings) override {
    EnsureCom();
    Close();
    if (settings.codec_type() != webrtc::kVideoCodecH264) {
      error_ = "not H.264";
      return false;
    }
    webrtc::RenderResolution size = settings.max_render_resolution();
    if (size.Valid()) {
      hint_width_ = size.Width();
      hint_height_ = size.Height();
    }
    if (!Open()) {
      RTC_LOG(LS_WARNING) << "Media Foundation H.264 decoder did not open ("
                          << error_ << "); using software";
      Close();
      return false;
    }
    RTC_LOG(LS_INFO) << "Media Foundation H.264 decoder: " << name_
                     << (use_gpu_ ? " on " + gpu_.adapter
                                  : std::string(" in software"));
    return true;
  }

  int32_t Decode(const webrtc::EncodedImage& input,
                 int64_t /* render_time_ms */) override {
    EnsureCom();
    if (!transform_ || !callback_)
      return WEBRTC_VIDEO_CODEC_UNINITIALIZED;
    if (failed_)
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    if (!input.data() || input.size() == 0)
      return WEBRTC_VIDEO_CODEC_ERR_PARAMETER;

    ComPtr<IMFSample> sample = InputSample(input);
    if (!sample)
      return Failed("input sample");
    LONGLONG time = ++count_ * kPictureTime;
    sample->SetSampleTime(time);
    sample->SetSampleDuration(kPictureTime);
    if (input.IsKey())
      sample->SetUINT32(MFSampleExtension_CleanPoint, TRUE);
    std::optional<webrtc::ColorSpace> color_space;
    if (input.ColorSpace())
      color_space = *input.ColorSpace();
    pending_.push_back(Pending{time, input.RtpTimestamp(), color_space});
    while (pending_.size() > 32)
      pending_.pop_front();

    HRESULT hr = transform_->ProcessInput(input_id_, sample.Get(), 0);
    if (hr == MF_E_NOTACCEPTING) {
      if (Drain() < 0)
        return Failed(error_);
      hr = transform_->ProcessInput(input_id_, sample.Get(), 0);
    }
    if (FAILED(hr))
      return Failed("ProcessInput " + Hex(hr));
    size_t before = decoded_;
    if (Drain() < 0)
      return Failed(error_);
    if (decoded_ == before) {
      if (++silent_ > kMaxSilent)
        return Failed("gives no pictures");
    } else {
      silent_ = 0;
    }
    return WEBRTC_VIDEO_CODEC_OK;
  }

  int32_t RegisterDecodeCompleteCallback(
      webrtc::DecodedImageCallback* callback) override {
    callback_ = callback;
    return WEBRTC_VIDEO_CODEC_OK;
  }

  int32_t Release() override {
    Close();
    return WEBRTC_VIDEO_CODEC_OK;
  }

  DecoderInfo GetDecoderInfo() const override {
    DecoderInfo info;
    std::string name = kImplementationName;
    if (use_gpu_)
      name += gpu_.adapter.empty() ? " (D3D11)"
                                   : " (D3D11 on " + gpu_.adapter + ")";
    else
      name += " (software)";
    info.implementation_name = name;
    info.is_hardware_accelerated = use_gpu_;
    return info;
  }

  const char* ImplementationName() const override {
    return kImplementationName;
  }

  // Self-test: the pictures still inside.
  void Flush() {
    if (!transform_)
      return;
    transform_->ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
    Drain();
  }

  const std::string& name() const { return name_; }
  const std::string& adapter() const { return gpu_.adapter; }
  const std::string& error() const { return error_; }

 private:
  struct Pending {
    LONGLONG time;
    uint32_t rtp_timestamp;
    std::optional<webrtc::ColorSpace> color_space;
  };

  bool Open() {
    if (!Mf()) {
      error_ = "no mfplat.dll";
      return false;
    }
    started_ = std::make_unique<MfStarted>();
    if (!started_->ok()) {
      error_ = "MFStartup failed";
      return false;
    }
    if (use_gpu_) {
      if (!CreateGpu(&gpu_, &error_))
        return false;
      if (gpu_.software_adapter) {
        error_ = "no GPU (" + gpu_.adapter + ")";
        return false;
      }
      if (!DecodesH264(gpu_.device.Get())) {
        error_ = gpu_.adapter + " has no H.264 decoder";
        return false;
      }
    }
    auto mfts = mf::FindMfts(
        MFT_CATEGORY_VIDEO_DECODER, MFVideoFormat_H264, MFVideoFormat_NV12,
        MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER);
    if (mfts.empty()) {
      error_ = "no H.264 decoder MFT";
      return false;
    }
    std::string errors;
    for (auto& activate : mfts) {
      if (OpenOne(activate.Get()))
        return true;
      errors += (errors.empty() ? "" : "; ") + FriendlyName(activate.Get()) +
                ": " + error_;
      CloseTransform();
    }
    error_ = errors;
    return false;
  }

  bool OpenOne(IMFActivate* activate) {
    const MfApi* mf = Mf();
    name_ = FriendlyName(activate);
    HRESULT hr = activate->ActivateObject(IID_PPV_ARGS(&transform_));
    if (FAILED(hr)) {
      error_ = "ActivateObject " + Hex(hr);
      return false;
    }
    activate_ = activate;
    ComPtr<IMFAttributes> attributes;
    transform_->GetAttributes(&attributes);
    if (attributes)
      attributes->SetUINT32(MF_LOW_LATENCY, TRUE);
    ComPtr<ICodecAPI> codec_api;
    if (SUCCEEDED(transform_.As(&codec_api)))
      SetBool(codec_api.Get(), CODECAPI_AVLowLatencyMode, true);
    if (use_gpu_) {
      UINT32 aware = 0;
      if (!attributes ||
          FAILED(attributes->GetUINT32(MF_SA_D3D11_AWARE, &aware)) || !aware) {
        error_ = "not Direct3D 11 aware";
        return false;
      }
      hr = transform_->ProcessMessage(
          MFT_MESSAGE_SET_D3D_MANAGER,
          reinterpret_cast<ULONG_PTR>(gpu_.manager.Get()));
      if (FAILED(hr)) {
        error_ = "Direct3D 11 device refused " + Hex(hr);
        return false;
      }
      d3d_set_ = true;
    }
    hr = transform_->GetStreamIDs(1, &input_id_, 1, &output_id_);
    if (hr == E_NOTIMPL) {
      input_id_ = 0;
      output_id_ = 0;
    } else if (FAILED(hr)) {
      error_ = "GetStreamIDs " + Hex(hr);
      return false;
    }

    ComPtr<IMFMediaType> input;
    if (FAILED(mf->create_media_type(&input))) {
      error_ = "MFCreateMediaType";
      return false;
    }
    input->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    input->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
    input->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    if (hint_width_ > 0 && hint_height_ > 0)
      MFSetAttributeSize(input.Get(), MF_MT_FRAME_SIZE, hint_width_,
                         hint_height_);
    hr = transform_->SetInputType(input_id_, input.Get(), 0);
    if (FAILED(hr)) {
      error_ = "SetInputType " + Hex(hr);
      return false;
    }
    if (!SetOutput())
      return false;
    hr = transform_->ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
    if (FAILED(hr)) {
      error_ = "begin streaming " + Hex(hr);
      return false;
    }
    hr = transform_->ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
    if (FAILED(hr)) {
      error_ = "start of stream " + Hex(hr);
      return false;
    }
    streaming_ = true;
    return true;
  }

  // NV12 out, at the size the MFT says now (again after a stream change).
  bool SetOutput() {
    for (DWORD i = 0;; ++i) {
      ComPtr<IMFMediaType> type;
      HRESULT hr = transform_->GetOutputAvailableType(output_id_, i, &type);
      if (FAILED(hr))
        break;
      GUID subtype = {};
      type->GetGUID(MF_MT_SUBTYPE, &subtype);
      if (subtype != MFVideoFormat_NV12)
        continue;
      hr = transform_->SetOutputType(output_id_, type.Get(), 0);
      if (FAILED(hr)) {
        error_ = "SetOutputType " + Hex(hr);
        return false;
      }
      ReadOutputType(type.Get());
      MFT_OUTPUT_STREAM_INFO info = {};
      hr = transform_->GetOutputStreamInfo(output_id_, &info);
      if (FAILED(hr)) {
        error_ = "GetOutputStreamInfo " + Hex(hr);
        return false;
      }
      provides_samples_ =
          (info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES |
                           MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES)) != 0;
      output_size_ = info.cbSize;
      return true;
    }
    error_ = "no NV12 output";
    return false;
  }

  void ReadOutputType(IMFMediaType* type) {
    UINT32 width = 0, height = 0;
    MFGetAttributeSize(type, MF_MT_FRAME_SIZE, &width, &height);
    coded_width_ = static_cast<int>(width);
    coded_height_ = static_cast<int>(height);
    crop_x_ = 0;
    crop_y_ = 0;
    crop_width_ = coded_width_;
    crop_height_ = coded_height_;
    MFVideoArea area = {};
    UINT32 got = 0;
    if (SUCCEEDED(type->GetBlob(MF_MT_MINIMUM_DISPLAY_APERTURE,
                                reinterpret_cast<UINT8*>(&area), sizeof(area),
                                &got)) &&
        got == sizeof(area) && area.Area.cx > 0 && area.Area.cy > 0) {
      crop_x_ = std::max<int>(0, area.OffsetX.value);
      crop_y_ = std::max<int>(0, area.OffsetY.value);
      crop_width_ = static_cast<int>(area.Area.cx);
      crop_height_ = static_cast<int>(area.Area.cy);
    }
    UINT32 stride = 0;
    if (SUCCEEDED(type->GetUINT32(MF_MT_DEFAULT_STRIDE, &stride)))
      stride_ = std::abs(static_cast<INT32>(stride));
    else
      stride_ = coded_width_;
  }

  void CloseTransform() {
    if (transform_) {
      if (streaming_) {
        transform_->ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
        transform_->ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
      }
      if (d3d_set_)
        transform_->ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, 0);
      ComPtr<IMFShutdown> shutdown;
      if (SUCCEEDED(transform_.As(&shutdown)))
        shutdown->Shutdown();
    }
    streaming_ = false;
    d3d_set_ = false;
    transform_.Reset();
    if (activate_) {
      activate_->ShutdownObject();
      activate_.Reset();
    }
  }

  void Close() {
    CloseTransform();
    staging_.Reset();
    staging_width_ = 0;
    staging_height_ = 0;
    std::string adapter = gpu_.adapter;
    gpu_ = Gpu();
    gpu_.adapter = adapter;  // still named in the stats
    started_.reset();
    pending_.clear();
    silent_ = 0;
    failed_ = false;
  }

  int32_t Failed(const std::string& why) {
    if (!failed_) {
      RTC_LOG(LS_WARNING) << "Media Foundation H.264 decoder (" << name_
                          << "): " << why << "; switching to software";
    }
    error_ = why;
    failed_ = true;
    return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
  }

  ComPtr<IMFSample> InputSample(const webrtc::EncodedImage& input) {
    const MfApi* mf = Mf();
    ComPtr<IMFMediaBuffer> buffer;
    ComPtr<IMFSample> sample;
    DWORD size = static_cast<DWORD>(input.size());
    if (FAILED(mf->create_memory_buffer(size, &buffer)))
      return nullptr;
    BYTE* data = nullptr;
    if (FAILED(buffer->Lock(&data, nullptr, nullptr)))
      return nullptr;
    memcpy(data, input.data(), size);
    buffer->Unlock();
    buffer->SetCurrentLength(size);
    if (FAILED(mf->create_sample(&sample)) ||
        FAILED(sample->AddBuffer(buffer.Get())))
      return nullptr;
    return sample;
  }

  // Every picture the MFT has now: 0 when it wants more input, -1 on error.
  int Drain() {
    for (int i = 0; i < 64; ++i) {
      int got = ProcessOutputOnce();
      if (got <= 0)
        return got;
    }
    return 0;
  }

  // 1: one out; 0: needs more input; 2: try again; -1: error.
  int ProcessOutputOnce() {
    const MfApi* mf = Mf();
    MFT_OUTPUT_DATA_BUFFER out = {};
    out.dwStreamID = output_id_;
    ComPtr<IMFSample> own;
    if (!provides_samples_) {
      DWORD size = std::max<DWORD>(
          output_size_,
          static_cast<DWORD>(std::max(stride_, coded_width_) *
                             coded_height_ * 3 / 2));
      if (size == 0)
        size = 1920 * 1088 * 3 / 2;
      ComPtr<IMFMediaBuffer> buffer;
      if (FAILED(mf->create_sample(&own)) ||
          FAILED(mf->create_memory_buffer(size, &buffer)) ||
          FAILED(own->AddBuffer(buffer.Get()))) {
        error_ = "output sample";
        return -1;
      }
      out.pSample = own.Get();
    }
    DWORD status = 0;
    HRESULT hr = transform_->ProcessOutput(0, 1, &out, &status);
    if (out.pEvents)
      out.pEvents->Release();
    ComPtr<IMFSample> sample;
    if (provides_samples_)
      sample.Attach(out.pSample);
    else
      sample = own;
    if (hr == MF_E_TRANSFORM_NEED_MORE_INPUT)
      return 0;
    if (hr == MF_E_TRANSFORM_STREAM_CHANGE) {
      if (!SetOutput())
        return -1;
      return 2;
    }
    if (FAILED(hr)) {
      error_ = "ProcessOutput " + Hex(hr);
      return -1;
    }
    if (!sample)
      return 2;
    if (!Deliver(sample.Get()))
      return -1;
    return 1;
  }

  bool Deliver(IMFSample* sample) {
    LONGLONG time = 0;
    sample->GetSampleTime(&time);
    ComPtr<IMFMediaBuffer> buffer;
    if (FAILED(sample->GetBufferByIndex(0, &buffer))) {
      error_ = "no output buffer";
      return false;
    }
    if (crop_width_ < 2 || crop_height_ < 2) {
      error_ = "no picture size";
      return false;
    }
    webrtc::scoped_refptr<webrtc::I420Buffer> i420 =
        webrtc::I420Buffer::Create(crop_width_, crop_height_);
    ComPtr<IMFDXGIBuffer> dxgi;
    if (SUCCEEDED(buffer.As(&dxgi))) {
      if (!CopyFromGpu(dxgi.Get(), i420.get()))
        return false;
    } else if (use_gpu_) {
      // DXVA turned this stream down and the MFT decodes in software:
      // FFmpeg does that as well, and says so in the stats.
      error_ = "decodes in software, not on the GPU";
      return false;
    } else if (!CopyFromMemory(buffer.Get(), i420.get())) {
      return false;
    }

    // Its picture: the one with its time, else the oldest still due.
    Pending picture{time, 0, std::nullopt};
    auto match = std::find_if(
        pending_.begin(), pending_.end(),
        [time](const Pending& p) { return p.time == time; });
    if (match != pending_.end()) {
      picture = *match;
      pending_.erase(pending_.begin(), match + 1);
    } else if (!pending_.empty()) {
      picture = pending_.front();
      pending_.pop_front();
    }

    webrtc::VideoFrame frame = webrtc::VideoFrame::Builder()
                                   .set_video_frame_buffer(i420)
                                   .set_rtp_timestamp(picture.rtp_timestamp)
                                   .set_color_space(picture.color_space)
                                   .build();
    ++decoded_;
    if (callback_)
      callback_->Decoded(frame, std::nullopt, std::nullopt);
    return true;
  }

  // The decoded texture (a slice of the decoder's texture array) to a
  // staging texture of ours, mapped and converted.
  bool CopyFromGpu(IMFDXGIBuffer* dxgi, webrtc::I420Buffer* i420) {
    ComPtr<ID3D11Texture2D> texture;
    UINT index = 0;
    if (FAILED(dxgi->GetResource(IID_PPV_ARGS(&texture))) ||
        FAILED(dxgi->GetSubresourceIndex(&index))) {
      error_ = "no texture";
      return false;
    }
    D3D11_TEXTURE2D_DESC desc = {};
    texture->GetDesc(&desc);
    if (desc.Format != DXGI_FORMAT_NV12) {
      error_ = "texture not NV12";
      return false;
    }
    ComPtr<ID3D11Device> device;
    texture->GetDevice(&device);
    ComPtr<ID3D11DeviceContext> context;
    device->GetImmediateContext(&context);
    if (!staging_ || staging_width_ != desc.Width ||
        staging_height_ != desc.Height) {
      staging_.Reset();
      D3D11_TEXTURE2D_DESC want = {};
      want.Width = desc.Width;
      want.Height = desc.Height;
      want.MipLevels = 1;
      want.ArraySize = 1;
      want.Format = DXGI_FORMAT_NV12;
      want.SampleDesc.Count = 1;
      want.Usage = D3D11_USAGE_STAGING;
      want.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
      HRESULT hr = device->CreateTexture2D(&want, nullptr, &staging_);
      if (FAILED(hr)) {
        error_ = "staging texture " + Hex(hr);
        return false;
      }
      staging_width_ = desc.Width;
      staging_height_ = desc.Height;
    }
    context->CopySubresourceRegion(staging_.Get(), 0, 0, 0, 0, texture.Get(),
                                   index, nullptr);
    D3D11_MAPPED_SUBRESOURCE mapped = {};
    HRESULT hr = context->Map(staging_.Get(), 0, D3D11_MAP_READ, 0, &mapped);
    if (FAILED(hr)) {
      error_ = "Map " + Hex(hr);
      return false;
    }
    int pitch = static_cast<int>(mapped.RowPitch);
    const uint8_t* y = static_cast<const uint8_t*>(mapped.pData);
    // NV12's chroma plane follows the luma plane, same pitch.
    const uint8_t* uv = y + static_cast<size_t>(pitch) * desc.Height;
    bool ok = Convert(y, uv, pitch, static_cast<int>(desc.Width),
                      static_cast<int>(desc.Height), i420);
    context->Unmap(staging_.Get(), 0);
    return ok;
  }

  bool CopyFromMemory(IMFMediaBuffer* buffer, webrtc::I420Buffer* i420) {
    ComPtr<IMF2DBuffer> two_d;
    BYTE* data = nullptr;
    LONG pitch = 0;
    if (SUCCEEDED(buffer->QueryInterface(IID_PPV_ARGS(&two_d))) &&
        SUCCEEDED(two_d->Lock2D(&data, &pitch))) {
      bool ok = pitch > 0 &&
                Convert(data, data + static_cast<size_t>(pitch) * coded_height_,
                        pitch, coded_width_, coded_height_, i420);
      two_d->Unlock2D();
      if (!ok && error_.empty())
        error_ = "2D buffer";
      return ok;
    }
    DWORD length = 0;
    if (FAILED(buffer->Lock(&data, nullptr, &length))) {
      error_ = "Lock";
      return false;
    }
    int stride = std::max(stride_, coded_width_);
    int plane_height = coded_height_;
    size_t whole = static_cast<size_t>(stride) * plane_height * 3 / 2;
    if (stride > 0 && length < whole)
      plane_height = static_cast<int>(length * 2 / (3 * stride));
    bool ok = stride > 0 &&
              Convert(data, data + static_cast<size_t>(stride) * plane_height,
                      stride, coded_width_, plane_height, i420);
    buffer->Unlock();
    return ok;
  }

  // NV12 (planes `width` x `height` at `pitch`) cropped to the picture.
  bool Convert(const uint8_t* y,
               const uint8_t* uv,
               int pitch,
               int width,
               int height,
               webrtc::I420Buffer* i420) {
    int x0 = crop_x_ & ~1;
    int y0 = crop_y_ & ~1;
    if (x0 + crop_width_ > width || y0 + crop_height_ > height ||
        pitch < width) {
      error_ = "picture " + std::to_string(crop_width_) + "x" +
               std::to_string(crop_height_) + " outside " +
               std::to_string(width) + "x" + std::to_string(height);
      return false;
    }
    libyuv::NV12ToI420(y + static_cast<size_t>(y0) * pitch + x0, pitch,
                       uv + static_cast<size_t>(y0 / 2) * pitch + x0, pitch,
                       i420->MutableDataY(), i420->StrideY(),
                       i420->MutableDataU(), i420->StrideU(),
                       i420->MutableDataV(), i420->StrideV(), crop_width_,
                       crop_height_);
    return true;
  }

  const bool use_gpu_;
  webrtc::DecodedImageCallback* callback_ = nullptr;
  std::unique_ptr<MfStarted> started_;
  Gpu gpu_;
  ComPtr<IMFActivate> activate_;
  ComPtr<IMFTransform> transform_;
  ComPtr<ID3D11Texture2D> staging_;
  UINT staging_width_ = 0;
  UINT staging_height_ = 0;
  DWORD input_id_ = 0;
  DWORD output_id_ = 0;
  bool streaming_ = false;
  bool d3d_set_ = false;
  bool provides_samples_ = false;
  DWORD output_size_ = 0;
  int hint_width_ = 0;
  int hint_height_ = 0;
  int coded_width_ = 0;
  int coded_height_ = 0;
  int crop_x_ = 0;
  int crop_y_ = 0;
  int crop_width_ = 0;
  int crop_height_ = 0;
  int stride_ = 0;
  LONGLONG count_ = 0;
  int silent_ = 0;
  bool failed_ = false;
  std::deque<Pending> pending_;
  std::string name_;
  std::string error_;
  size_t decoded_ = 0;
};

struct Probe {
  bool supported = false;
  std::string adapter;
  std::string why;
};

// Opens the decoder on the GPU once: a Direct3D 11 device that decodes
// H.264, and the MFT taking it.
const Probe& ProbeGpu() {
  static const Probe probe = []() {
    Probe found;
    EnsureCom();
    MfStarted started;
    if (!started.ok()) {
      found.why = "no Media Foundation";
      return found;
    }
    MediaFoundationH264Decoder decoder(true);
    webrtc::VideoDecoder::Settings settings;
    settings.set_codec_type(webrtc::kVideoCodecH264);
    if (decoder.Configure(settings)) {
      found.supported = true;
      found.adapter = decoder.adapter();
      RTC_LOG(LS_INFO) << "Media Foundation hardware H.264 decoder: "
                       << decoder.name() << " on " << found.adapter;
    } else {
      found.why = decoder.error();
      RTC_LOG(LS_INFO) << "Media Foundation: no hardware H.264 decoder: "
                       << found.why;
    }
    decoder.Release();
    return found;
  }();
  return probe;
}

}  // namespace

MediaFoundationVideoDecoderFactory::MediaFoundationVideoDecoderFactory(
    bool software_h264)
    : software_h264_(software_h264) {
  using webrtc::H264Level;
  using webrtc::H264Profile;
  for (const char* mode : {"1", "0"}) {
    for (H264Profile profile :
         {H264Profile::kProfileConstrainedBaseline,
          H264Profile::kProfileBaseline, H264Profile::kProfileMain,
          H264Profile::kProfileConstrainedHigh, H264Profile::kProfileHigh}) {
      formats_.push_back(
          webrtc::CreateH264Format(profile, H264Level::kLevel5_1, mode));
    }
  }
}

bool MediaFoundationVideoDecoderFactory::IsSupported() {
  return ProbeGpu().supported;
}

std::string MediaFoundationVideoDecoderFactory::DecoderName() {
  return ProbeGpu().adapter;
}

std::string MediaFoundationVideoDecoderFactory::Unavailable() {
  return ProbeGpu().why;
}

std::vector<webrtc::SdpVideoFormat>
MediaFoundationVideoDecoderFactory::GetSupportedFormats() const {
  return formats_;
}

std::unique_ptr<webrtc::VideoDecoder> MediaFoundationVideoDecoderFactory::Create(
    const webrtc::Environment& env,
    const webrtc::SdpVideoFormat& format) {
  if (!absl::EqualsIgnoreCase(format.name, "H264"))
    return nullptr;
  auto hardware = std::make_unique<MediaFoundationH264Decoder>(true);
  if (!software_h264_)
    return hardware;
  // FFmpeg takes over if the GPU decoder does not open or stops working.
  return webrtc::CreateVideoDecoderSoftwareFallbackWrapper(
      env, webrtc::H264Decoder::Create(), std::move(hardware));
}

namespace {
// Counts and checks what the decoder gives (the self-test).
class CheckingCallback : public webrtc::DecodedImageCallback {
 public:
  int32_t Decoded(webrtc::VideoFrame& frame) override {
    ++pictures;
    width = frame.width();
    height = frame.height();
    auto i420 = frame.video_frame_buffer()->ToI420();
    if (!i420)
      return 0;
    int picture = static_cast<int>((frame.rtp_timestamp() - 1000) / 3000);
    // Luma against the picture sent, chroma against grey (128).
    double luma = 0;
    for (int y = 0; y < i420->height(); ++y)
      for (int x = 0; x < i420->width(); ++x)
        luma += std::abs(int(i420->DataY()[y * i420->StrideY() + x]) -
                         int(MediaFoundationTestLuma(x, y, picture)));
    luma /= double(i420->width()) * i420->height();
    double chroma = 0;
    int cw = i420->ChromaWidth(), ch = i420->ChromaHeight();
    for (int y = 0; y < ch; ++y)
      for (int x = 0; x < cw; ++x)
        chroma += std::abs(int(i420->DataU()[y * i420->StrideU() + x]) - 128) +
                  std::abs(int(i420->DataV()[y * i420->StrideV() + x]) - 128);
    chroma /= 2.0 * cw * ch;
    luma_error += luma;
    chroma_error += chroma;
    return 0;
  }

  int pictures = 0;
  int width = 0;
  int height = 0;
  double luma_error = 0;
  double chroma_error = 0;
};
}  // namespace

std::string MediaFoundationH264DecodeSelfTest(bool gpu, int frames) {
  EnsureCom();
  if (!Mf())
    return "unavailable: no mfplat.dll (Media Foundation)";
  if (gpu && !ProbeGpu().supported)
    return "unavailable: " + ProbeGpu().why;
  constexpr int kWidth = 640;
  constexpr int kHeight = 360;
  std::vector<webrtc::EncodedImage> images;
  std::string encoded =
      MediaFoundationH264Encode(true, kWidth, kHeight, frames, &images);
  if (encoded.rfind("unavailable:", 0) == 0)
    return "unavailable: encoder: " + encoded.substr(13);
  if (encoded.rfind("ok:", 0) != 0)
    return "error: encoder: " + encoded;

  MediaFoundationH264Decoder decoder(gpu);
  CheckingCallback callback;
  decoder.RegisterDecodeCompleteCallback(&callback);
  webrtc::VideoDecoder::Settings settings;
  settings.set_codec_type(webrtc::kVideoCodecH264);
  if (!decoder.Configure(settings)) {
    std::string error = decoder.error();
    if (!gpu && error.rfind("no H.264 decoder", 0) == 0)
      return "unavailable: " + error;
    return "error: Configure: " + error;
  }
  for (size_t i = 0; i < images.size(); ++i) {
    int32_t r = decoder.Decode(images[i], static_cast<int64_t>(0));
    if (r != WEBRTC_VIDEO_CODEC_OK)
      return "error: " + decoder.name() + ": Decode " + std::to_string(r) +
             " at picture " + std::to_string(i) + ": " + decoder.error();
  }
  decoder.Flush();
  std::string where =
      gpu ? " on " + decoder.adapter() : std::string(" in software");
  double luma = callback.pictures ? callback.luma_error / callback.pictures : 0;
  double chroma =
      callback.pictures ? callback.chroma_error / callback.pictures : 0;
  char errors[64];
  snprintf(errors, sizeof(errors), "luma error %.1f, chroma error %.1f", luma,
           chroma);
  std::string report = decoder.name() + where + ": " +
                       std::to_string(images.size()) + " pictures in, " +
                       std::to_string(callback.pictures) + " out, " +
                       std::to_string(callback.width) + "x" +
                       std::to_string(callback.height) + ", " + errors;
  decoder.Release();
  if (callback.pictures + 2 < static_cast<int>(images.size()) ||
      callback.width != kWidth || callback.height != kHeight ||
      luma > 16.0 || chroma > 8.0)
    return "error: " + report;
  return "ok: " + report;
}

}  // namespace livekit_ffi
