/*
 * Morpheus patch: hardware H.264 encoding on Windows through Media
 * Foundation. See mf_h264_encoder.h.
 *
 * The encoder is a webrtc::VideoEncoder around an H.264 encoder MFT fed
 * NV12 pictures in system memory (as FFmpeg's h264_mf does). Hardware MFTs
 * are asynchronous: they ask for input (METransformNeedInput) and announce
 * output (METransformHaveOutput) through their event queue, read here with
 * BeginGetEvent so waits are short and bounded. Microsoft's software MFT
 * (synchronous) goes through the same class for the CI self-test.
 *
 * The factory wraps it in libwebrtc's software fallback: if the MFT does
 * not open, or stops giving pictures, OpenH264 takes over.
 */

#include "mf_h264_encoder.h"

#include <algorithm>
#include <atomic>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <mutex>
#include <optional>
#include <span>

#include "api/environment/environment_factory.h"
#include "api/video/i420_buffer.h"
#include "api/video/video_frame.h"
#include "api/video_codecs/video_encoder_software_fallback_wrapper.h"
#include "common_video/h264/h264_bitstream_parser.h"
#include "modules/video_coding/codecs/h264/include/h264.h"
#include "modules/video_coding/codecs/interface/common_constants.h"
#include "modules/video_coding/include/video_codec_interface.h"
#include "modules/video_coding/include/video_error_codes.h"
#include "rtc_base/logging.h"
#include "third_party/libyuv/include/libyuv/convert_from.h"

#include <windows.h>
// The Media Foundation GUIDs this file uses, defined here (selectany), so
// nothing links mfuuid.lib or mfplat.dll.
#include <initguid.h>
#include <strmif.h>
#include <codecapi.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <mftransform.h>
#include <wrl/client.h>

#include "mf_common.h"

using Microsoft::WRL::ComPtr;

namespace livekit_ffi {

namespace {

constexpr char kImplementationName[] = "Media Foundation H264 Encoder";
// How long a picture may wait for the MFT to take it, and for its output.
constexpr DWORD kInputWaitMs = 100;
constexpr DWORD kOutputWaitMs = 30;
// Pictures in a row the MFT would not take before OpenH264 takes over.
constexpr int kMaxStalls = 15;
// Pictures a forced key picture may take to come out before it is asked
// for again.
constexpr int kKeyWait = 10;

using mf::EnsureCom;
using mf::FriendlyName;
using mf::Hex;
using mf::Mf;
using mf::MfApi;
using mf::MfStarted;

// The encoder MFTs taking NV12 and giving H.264: the GPU's (hardware) or
// the synchronous software ones.
std::vector<ComPtr<IMFActivate>> EncoderMfts(bool hardware) {
  return mf::FindMfts(
      MFT_CATEGORY_VIDEO_ENCODER, MFVideoFormat_NV12, MFVideoFormat_H264,
      MFT_ENUM_FLAG_SORTANDFILTER |
          (hardware ? MFT_ENUM_FLAG_HARDWARE : MFT_ENUM_FLAG_SYNCMFT));
}

void SetUint(ICodecAPI* api, const GUID& what, ULONG value) {
  if (!api)
    return;
  VARIANT v;
  memset(&v, 0, sizeof(v));
  v.vt = VT_UI4;
  v.ulVal = value;
  api->SetValue(&what, &v);
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

// Whether an Annex B stream has a NAL unit of `type`.
bool HasNal(const uint8_t* data, size_t size, uint8_t type) {
  for (size_t i = 0; i + 3 < size; ++i) {
    if (data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1) {
      if ((data[i + 3] & 0x1f) == type)
        return true;
      i += 2;
    }
  }
  return false;
}

// An async MFT's events, read with BeginGetEvent and handed over through a
// queue and a Win32 event (precise waits, unlike polling with Sleep).
class EventPump final : public IMFAsyncCallback {
 public:
  struct Event {
    MediaEventType type;
    HRESULT status;
  };

  explicit EventPump(IMFMediaEventGenerator* events) : events_(events) {
    signal_ = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  }

  bool Start() {
    HRESULT hr = events_->BeginGetEvent(this, nullptr);
    return SUCCEEDED(hr);
  }

  void Stop() { stopped_ = true; }

  // The next event, waiting up to `ms`; false on timeout (`failed` set when
  // the queue broke).
  bool Next(DWORD ms, Event* event, HRESULT* failed) {
    ULONGLONG deadline = GetTickCount64() + ms;
    while (true) {
      {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!queue_.empty()) {
          *event = queue_.front();
          queue_.pop_front();
          return true;
        }
        if (FAILED(failed_)) {
          *failed = failed_;
          return false;
        }
      }
      ULONGLONG now = GetTickCount64();
      if (now >= deadline)
        return false;
      WaitForSingleObject(signal_, static_cast<DWORD>(deadline - now));
    }
  }

  // IMFAsyncCallback
  STDMETHODIMP GetParameters(DWORD*, DWORD*) override { return E_NOTIMPL; }
  STDMETHODIMP Invoke(IMFAsyncResult* result) override {
    ComPtr<IMFMediaEvent> event;
    HRESULT hr = events_->EndGetEvent(result, &event);
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (FAILED(hr)) {
        failed_ = hr;
      } else {
        Event e{MEUnknown, S_OK};
        event->GetType(&e.type);
        event->GetStatus(&e.status);
        queue_.push_back(e);
      }
    }
    SetEvent(signal_);
    if (SUCCEEDED(hr) && !stopped_) {
      hr = events_->BeginGetEvent(this, nullptr);
      if (FAILED(hr)) {
        std::lock_guard<std::mutex> lock(mutex_);
        failed_ = hr;
        SetEvent(signal_);
      }
    }
    return S_OK;
  }

  // IUnknown
  STDMETHODIMP QueryInterface(REFIID riid, void** out) override {
    if (riid == __uuidof(IUnknown) || riid == __uuidof(IMFAsyncCallback)) {
      *out = static_cast<IMFAsyncCallback*>(this);
      AddRef();
      return S_OK;
    }
    *out = nullptr;
    return E_NOINTERFACE;
  }
  STDMETHODIMP_(ULONG) AddRef() override { return ++refs_; }
  STDMETHODIMP_(ULONG) Release() override {
    ULONG left = --refs_;
    if (left == 0)
      delete this;
    return left;
  }

 private:
  ~EventPump() {
    if (signal_)
      CloseHandle(signal_);
  }

  ComPtr<IMFMediaEventGenerator> events_;
  HANDLE signal_ = nullptr;
  std::mutex mutex_;
  std::deque<Event> queue_;
  HRESULT failed_ = S_OK;
  std::atomic<bool> stopped_{false};
  std::atomic<ULONG> refs_{1};
};

class MediaFoundationH264Encoder : public webrtc::VideoEncoder {
 public:
  MediaFoundationH264Encoder(const webrtc::SdpVideoFormat& format,
                             bool software)
      : software_(software),
        packetization_mode_(
            webrtc::H264EncoderSettings::Parse(format).packetization_mode) {}

  ~MediaFoundationH264Encoder() override { Close(); }

  int32_t InitEncode(const webrtc::VideoCodec* codec,
                     const Settings& settings) override {
    EnsureCom();
    if (!codec || codec->codecType != webrtc::kVideoCodecH264 ||
        codec->width < 2 || codec->height < 2 || codec->maxFramerate == 0) {
      return WEBRTC_VIDEO_CODEC_ERR_PARAMETER;
    }
    if ((codec->width & 1) || (codec->height & 1)) {
      error_ = "odd picture size";
      return WEBRTC_VIDEO_CODEC_ERR_PARAMETER;
    }
    Close();
    width_ = codec->width;
    height_ = codec->height;
    fps_ = std::max<uint32_t>(1, codec->maxFramerate);
    bps_ = std::max<uint32_t>(100000, codec->startBitrate * 1000);
    if (!Open()) {
      RTC_LOG(LS_WARNING) << "Media Foundation H.264 encoder did not open ("
                          << error_ << "); using software";
      Close();
      return WEBRTC_VIDEO_CODEC_ERROR;
    }
    sending_ = true;
    key_pending_ = true;
    RTC_LOG(LS_INFO) << "Media Foundation H.264 encoder: " << name_ << ", "
                     << width_ << "x" << height_ << "@" << fps_ << ", "
                     << (async_ ? "async" : "sync");
    return WEBRTC_VIDEO_CODEC_OK;
  }

  int32_t RegisterEncodeCompleteCallback(
      webrtc::EncodedImageCallback* callback) override {
    callback_ = callback;
    return WEBRTC_VIDEO_CODEC_OK;
  }

  int32_t Release() override {
    Close();
    return WEBRTC_VIDEO_CODEC_OK;
  }

  int32_t Encode(const webrtc::VideoFrame& frame,
                 const std::vector<webrtc::VideoFrameType>* types) override {
    EnsureCom();
    if (!transform_ || !callback_)
      return WEBRTC_VIDEO_CODEC_UNINITIALIZED;
    if (types && !types->empty() &&
        (*types)[0] == webrtc::VideoFrameType::kEmptyFrame)
      return WEBRTC_VIDEO_CODEC_NO_OUTPUT;
    if (!sending_)
      return WEBRTC_VIDEO_CODEC_NO_OUTPUT;
    bool key = key_pending_ ||
               (types && std::find(types->begin(), types->end(),
                                   webrtc::VideoFrameType::kVideoFrameKey) !=
                             types->end());
    webrtc::scoped_refptr<webrtc::I420BufferInterface> i420 =
        frame.video_frame_buffer()->ToI420();
    if (!i420)
      return WEBRTC_VIDEO_CODEC_ENCODER_FAILURE;
    if (i420->width() != width_ || i420->height() != height_) {
      // libwebrtc reinitialises on a size change; a stray one goes to
      // software rather than being encoded at the wrong size.
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }

    // Outputs that came in since the last picture.
    if (async_ && !PumpEvents(0, false))
      return Failed("event queue");

    if (async_ && need_input_ == 0 && !PumpEvents(kInputWaitMs, true)) {
      return Failed("event queue");
    }
    if (async_ && need_input_ == 0) {
      // The MFT is still busy: drop this picture (a key frame stays due).
      if (++stalls_ > kMaxStalls)
        return Failed("stopped taking pictures");
      key_pending_ = key_pending_ || key;
      callback_->OnFrameDropped(frame.rtp_timestamp(), 0, true);
      return WEBRTC_VIDEO_CODEC_OK;
    }
    stalls_ = 0;

    ComPtr<IMFSample> sample = Picture(*i420);
    if (!sample)
      return Failed("no sample");
    LONGLONG time = frame.timestamp_us() * 10;
    if (time <= last_time_)
      time = last_time_ + 1;
    last_time_ = time;
    sample->SetSampleTime(time);
    sample->SetSampleDuration(10000000LL / fps_);
    if (key) {
      SetUint(codec_api_.Get(), CODECAPI_AVEncVideoForceKeyFrame, 1);
      key_pending_ = false;
      awaiting_key_ = kKeyWait;
    } else if (awaiting_key_ > 0 && --awaiting_key_ == 0) {
      // Asked for, never came: ask again with the next picture.
      key_pending_ = true;
    }

    HRESULT hr = transform_->ProcessInput(input_id_, sample.Get(), 0);
    if (hr == MF_E_NOTACCEPTING && !async_) {
      if (!DrainSync())
        return Failed("output");
      hr = transform_->ProcessInput(input_id_, sample.Get(), 0);
    }
    if (FAILED(hr))
      return Failed("ProcessInput " + Hex(hr));
    if (async_)
      --need_input_;
    pending_.push_back(
        Pending{time, frame.rtp_timestamp(), frame.render_time_ms(),
                frame.color_space()});
    while (pending_.size() > 64)
      pending_.pop_front();

    if (async_) {
      // Usually ready within a few milliseconds; later ones go out with
      // the next picture.
      size_t before = delivered_;
      ULONGLONG deadline = GetTickCount64() + kOutputWaitMs;
      while (delivered_ == before) {
        ULONGLONG now = GetTickCount64();
        if (now >= deadline)
          break;
        if (!PumpEvents(static_cast<DWORD>(deadline - now), false, true))
          return Failed("event queue");
      }
    } else if (!DrainSync()) {
      return Failed("output");
    }
    return failed_ ? WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE
                   : WEBRTC_VIDEO_CODEC_OK;
  }

  void SetRates(const RateControlParameters& parameters) override {
    uint32_t bps = parameters.bitrate.get_sum_bps();
    if (bps == 0) {
      sending_ = false;
      return;
    }
    if (!sending_)
      key_pending_ = true;
    sending_ = true;
    if (parameters.framerate_fps >= 1.0)
      fps_ = static_cast<uint32_t>(parameters.framerate_fps + 0.5);
    if (bps != bps_) {
      bps_ = bps;
      SetUint(codec_api_.Get(), CODECAPI_AVEncCommonMeanBitRate, bps_);
    }
  }

  EncoderInfo GetEncoderInfo() const override {
    EncoderInfo info;
    info.supports_native_handle = false;
    info.implementation_name =
        name_.empty() ? std::string(kImplementationName)
                      : std::string(kImplementationName) + " (" + name_ + ")";
    info.scaling_settings = VideoEncoder::ScalingSettings::kOff;
    info.is_hardware_accelerated = !software_;
    info.supports_simulcast = false;
    info.requested_resolution_alignment = 2;
    info.preferred_pixel_formats = {webrtc::VideoFrameBuffer::Type::kI420};
    return info;
  }

  // Self-test: wait up to `ms` for the outputs still due.
  void Flush(DWORD ms) {
    if (!transform_)
      return;
    if (!async_) {
      DrainSync();
      return;
    }
    ULONGLONG deadline = GetTickCount64() + ms;
    while (!pending_.empty() && !failed_) {
      ULONGLONG now = GetTickCount64();
      if (now >= deadline)
        break;
      if (!PumpEvents(static_cast<DWORD>(deadline - now), false, true))
        break;
    }
  }

  const std::string& name() const { return name_; }
  const std::string& error() const { return error_; }
  size_t delivered() const { return delivered_; }
  size_t keyframes() const { return keyframes_; }
  size_t bytes() const { return bytes_; }

 private:
  struct Pending {
    LONGLONG time;
    uint32_t rtp_timestamp;
    int64_t render_time_ms;
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
    auto mfts = EncoderMfts(!software_);
    if (mfts.empty()) {
      error_ = software_ ? "no software H.264 MFT" : "no hardware H.264 MFT";
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
    UINT32 async = 0;
    if (SUCCEEDED(transform_->GetAttributes(&attributes)) &&
        SUCCEEDED(attributes->GetUINT32(MF_TRANSFORM_ASYNC, &async)) && async) {
      hr = attributes->SetUINT32(MF_TRANSFORM_ASYNC_UNLOCK, TRUE);
      if (FAILED(hr)) {
        error_ = "async unlock " + Hex(hr);
        return false;
      }
      async_ = true;
    }
    hr = transform_->GetStreamIDs(1, &input_id_, 1, &output_id_);
    if (hr == E_NOTIMPL) {
      input_id_ = 0;
      output_id_ = 0;
    } else if (FAILED(hr)) {
      error_ = "GetStreamIDs " + Hex(hr);
      return false;
    }
    transform_.As(&codec_api_);
    if (codec_api_) {
      SetUint(codec_api_.Get(), CODECAPI_AVEncCommonRateControlMode,
              eAVEncCommonRateControlMode_CBR);
      SetUint(codec_api_.Get(), CODECAPI_AVEncCommonMeanBitRate, bps_);
      SetBool(codec_api_.Get(), CODECAPI_AVLowLatencyMode, true);
      SetUint(codec_api_.Get(), CODECAPI_AVEncMPVDefaultBPictureCount, 0);
      SetUint(codec_api_.Get(), CODECAPI_AVEncMPVGOPSize, fps_ * 5);
    }

    // Output first (encoders want it before the input).
    const UINT32 profiles[] = {eAVEncH264VProfile_Base,
                               eAVEncH264VProfile_ConstrainedBase,
                               eAVEncH264VProfile_Main};
    hr = E_FAIL;
    for (UINT32 profile : profiles) {
      ComPtr<IMFMediaType> type;
      if (FAILED(mf->create_media_type(&type)))
        break;
      type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
      type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
      type->SetUINT32(MF_MT_AVG_BITRATE, bps_);
      MFSetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, width_, height_);
      MFSetAttributeRatio(type.Get(), MF_MT_FRAME_RATE, fps_, 1);
      MFSetAttributeRatio(type.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
      type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
      type->SetUINT32(MF_MT_MPEG2_PROFILE, profile);
      hr = transform_->SetOutputType(output_id_, type.Get(), 0);
      if (SUCCEEDED(hr))
        break;
    }
    if (FAILED(hr)) {
      error_ = "SetOutputType " + Hex(hr);
      return false;
    }

    ComPtr<IMFMediaType> input;
    if (FAILED(mf->create_media_type(&input))) {
      error_ = "MFCreateMediaType";
      return false;
    }
    input->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    input->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_NV12);
    MFSetAttributeSize(input.Get(), MF_MT_FRAME_SIZE, width_, height_);
    MFSetAttributeRatio(input.Get(), MF_MT_FRAME_RATE, fps_, 1);
    MFSetAttributeRatio(input.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    input->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    input->SetUINT32(MF_MT_DEFAULT_STRIDE, width_);
    hr = transform_->SetInputType(input_id_, input.Get(), 0);
    if (FAILED(hr)) {
      // Some want one of the types they offer, filled in.
      for (DWORD i = 0; FAILED(hr); ++i) {
        ComPtr<IMFMediaType> offered;
        if (FAILED(transform_->GetInputAvailableType(input_id_, i, &offered)))
          break;
        GUID subtype = {};
        offered->GetGUID(MF_MT_SUBTYPE, &subtype);
        if (subtype != MFVideoFormat_NV12)
          continue;
        MFSetAttributeSize(offered.Get(), MF_MT_FRAME_SIZE, width_, height_);
        MFSetAttributeRatio(offered.Get(), MF_MT_FRAME_RATE, fps_, 1);
        hr = transform_->SetInputType(input_id_, offered.Get(), 0);
      }
    }
    if (FAILED(hr)) {
      error_ = "SetInputType " + Hex(hr);
      return false;
    }
    if (!ReadOutputInfo())
      return false;

    if (async_) {
      ComPtr<IMFMediaEventGenerator> events;
      if (FAILED(transform_.As(&events))) {
        error_ = "no event queue";
        return false;
      }
      pump_.Attach(new EventPump(events.Get()));
      if (!pump_->Start()) {
        error_ = "BeginGetEvent";
        return false;
      }
    }
    transform_->ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
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

  bool ReadOutputInfo() {
    MFT_OUTPUT_STREAM_INFO info = {};
    HRESULT hr = transform_->GetOutputStreamInfo(output_id_, &info);
    if (FAILED(hr)) {
      error_ = "GetOutputStreamInfo " + Hex(hr);
      return false;
    }
    provides_samples_ =
        (info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES |
                         MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES)) != 0;
    output_size_ = std::max<DWORD>(info.cbSize, width_ * height_ * 3 / 2);
    return true;
  }

  void CloseTransform() {
    if (pump_)
      pump_->Stop();
    if (transform_) {
      if (streaming_) {
        transform_->ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
        transform_->ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
      }
      // Ends the event queue: a pending BeginGetEvent lets go of the pump.
      ComPtr<IMFShutdown> shutdown;
      if (SUCCEEDED(transform_.As(&shutdown)))
        shutdown->Shutdown();
    }
    streaming_ = false;
    pump_.Reset();
    codec_api_.Reset();
    transform_.Reset();
    if (activate_) {
      activate_->ShutdownObject();
      activate_.Reset();
    }
    async_ = false;
    need_input_ = 0;
  }

  void Close() {
    CloseTransform();
    started_.reset();
    pending_.clear();
    sequence_header_.clear();
    last_time_ = -1;
    stalls_ = 0;
    awaiting_key_ = 0;
    failed_ = false;
  }

  int32_t Failed(const std::string& why) {
    if (!failed_) {
      RTC_LOG(LS_WARNING) << "Media Foundation H.264 encoder (" << name_
                          << "): " << why << "; switching to software";
    }
    error_ = why;
    failed_ = true;
    return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
  }

  // Handles the MFT's events for up to `ms`: until it asks for a picture
  // (`until_input`), one output went out (`until_output`), or the time is
  // up. False when the queue broke or the MFT reported an error.
  bool PumpEvents(DWORD ms, bool until_input, bool until_output = false) {
    ULONGLONG deadline = GetTickCount64() + ms;
    size_t before = delivered_;
    while (true) {
      ULONGLONG now = GetTickCount64();
      DWORD wait = now >= deadline ? 0 : static_cast<DWORD>(deadline - now);
      EventPump::Event event;
      HRESULT broken = S_OK;
      if (!pump_->Next(wait, &event, &broken)) {
        if (FAILED(broken)) {
          error_ = "events " + Hex(broken);
          return false;
        }
        return true;
      }
      switch (event.type) {
        case METransformNeedInput:
          ++need_input_;
          break;
        case METransformHaveOutput:
          if (!TakeOutput())
            return false;
          break;
        case MEError:
          error_ = "MFT error " + Hex(event.status);
          return false;
        default:
          break;
      }
      if (until_input && need_input_ > 0)
        return true;
      if (until_output && delivered_ != before)
        return true;
    }
  }

  // A synchronous MFT: every output it has now.
  bool DrainSync() {
    for (int i = 0; i < 16; ++i) {
      int got = ProcessOutputOnce();
      if (got < 0)
        return false;
      if (got == 0)
        return true;
    }
    return true;
  }

  bool TakeOutput() {
    for (int i = 0; i < 4; ++i) {
      int got = ProcessOutputOnce();
      if (got != 2)  // 2: the output type changed, ask again
        return got >= 0;
    }
    return true;
  }

  // 1: one out; 0: nothing ready; 2: try again; -1: error.
  int ProcessOutputOnce() {
    const MfApi* mf = Mf();
    MFT_OUTPUT_DATA_BUFFER out = {};
    out.dwStreamID = output_id_;
    ComPtr<IMFSample> own;
    if (!provides_samples_) {
      ComPtr<IMFMediaBuffer> buffer;
      if (FAILED(mf->create_sample(&own)) ||
          FAILED(mf->create_memory_buffer(output_size_, &buffer)) ||
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
      ComPtr<IMFMediaType> type;
      if (FAILED(transform_->GetOutputAvailableType(output_id_, 0, &type)) ||
          FAILED(transform_->SetOutputType(output_id_, type.Get(), 0)) ||
          !ReadOutputInfo()) {
        error_ = "output type change";
        return -1;
      }
      sequence_header_.clear();
      return 2;
    }
    if (FAILED(hr)) {
      error_ = "ProcessOutput " + Hex(hr);
      return -1;
    }
    if (!sample)
      return 0;
    Deliver(sample.Get());
    return 1;
  }

  void Deliver(IMFSample* sample) {
    LONGLONG time = 0;
    sample->GetSampleTime(&time);
    ComPtr<IMFMediaBuffer> buffer;
    if (FAILED(sample->ConvertToContiguousBuffer(&buffer)))
      return;
    BYTE* data = nullptr;
    DWORD length = 0;
    if (FAILED(buffer->Lock(&data, nullptr, &length)))
      return;
    std::vector<uint8_t> bytes(data, data + length);
    buffer->Unlock();
    if (bytes.empty())
      return;

    // Its picture: the one with its time (pictures the MFT skipped before
    // it are forgotten), else the oldest still due.
    Pending picture{time, 0, 0, std::nullopt};
    auto match = std::find_if(pending_.begin(), pending_.end(),
                              [time](const Pending& p) { return p.time == time; });
    if (match != pending_.end()) {
      picture = *match;
      // Pictures before it the MFT did not encode.
      for (auto skipped = pending_.begin(); skipped != match; ++skipped) {
        if (callback_)
          callback_->OnFrameDropped(skipped->rtp_timestamp, 0, true);
      }
      pending_.erase(pending_.begin(), match + 1);
    } else if (!pending_.empty()) {
      // An MFT that does not keep the times: pictures come out in order.
      picture = pending_.front();
      pending_.pop_front();
    }

    bool idr = HasNal(bytes.data(), bytes.size(), 5);
    if (idr && !HasNal(bytes.data(), bytes.size(), 7)) {
      // A key picture without its SPS/PPS: put the stream's in front.
      if (sequence_header_.empty())
        ReadSequenceHeader();
      bytes.insert(bytes.begin(), sequence_header_.begin(),
                   sequence_header_.end());
    }

    webrtc::EncodedImage image;
    image.SetEncodedData(
        webrtc::EncodedImageBuffer::Create(bytes.data(), bytes.size()));
    image._encodedWidth = width_;
    image._encodedHeight = height_;
    image.SetRtpTimestamp(picture.rtp_timestamp);
    image.capture_time_ms_ = picture.render_time_ms;
    if (picture.color_space)
      image.SetColorSpace(picture.color_space);
    image._frameType = idr ? webrtc::VideoFrameType::kVideoFrameKey
                           : webrtc::VideoFrameType::kVideoFrameDelta;
    parser_.ParseBitstream(
        std::span<const uint8_t>(image.data(), image.size()));
    image.qp_ = parser_.GetLastSliceQp().value_or(-1);

    webrtc::CodecSpecificInfo info;
    info.codecType = webrtc::kVideoCodecH264;
    info.codecSpecific.H264.packetization_mode = packetization_mode_;
    info.codecSpecific.H264.temporal_idx = webrtc::kNoTemporalIdx;
    info.codecSpecific.H264.base_layer_sync = false;
    info.codecSpecific.H264.idr_frame = idr;
    if (idr) {
      awaiting_key_ = 0;
      ++keyframes_;
    }
    ++delivered_;
    bytes_ += bytes.size();
    if (callback_)
      callback_->OnEncodedImage(image, &info);
  }

  void ReadSequenceHeader() {
    ComPtr<IMFMediaType> type;
    if (FAILED(transform_->GetOutputCurrentType(output_id_, &type)))
      return;
    UINT32 size = 0;
    if (FAILED(type->GetBlobSize(MF_MT_MPEG_SEQUENCE_HEADER, &size)) ||
        size == 0)
      return;
    sequence_header_.resize(size);
    if (FAILED(type->GetBlob(MF_MT_MPEG_SEQUENCE_HEADER,
                             sequence_header_.data(), size, &size)))
      sequence_header_.clear();
    else
      sequence_header_.resize(size);
  }

  ComPtr<IMFSample> Picture(const webrtc::I420BufferInterface& i420) {
    const MfApi* mf = Mf();
    DWORD size = width_ * height_ * 3 / 2;
    ComPtr<IMFMediaBuffer> buffer;
    ComPtr<IMFSample> sample;
    if (FAILED(mf->create_memory_buffer(size, &buffer)))
      return nullptr;
    BYTE* data = nullptr;
    if (FAILED(buffer->Lock(&data, nullptr, nullptr)))
      return nullptr;
    libyuv::I420ToNV12(i420.DataY(), i420.StrideY(), i420.DataU(),
                       i420.StrideU(), i420.DataV(), i420.StrideV(), data,
                       width_, data + width_ * height_, width_, width_,
                       height_);
    buffer->Unlock();
    buffer->SetCurrentLength(size);
    if (FAILED(mf->create_sample(&sample)) ||
        FAILED(sample->AddBuffer(buffer.Get())))
      return nullptr;
    return sample;
  }

  const bool software_;
  const webrtc::H264PacketizationMode packetization_mode_;
  webrtc::EncodedImageCallback* callback_ = nullptr;
  std::unique_ptr<MfStarted> started_;
  ComPtr<IMFActivate> activate_;
  ComPtr<IMFTransform> transform_;
  ComPtr<ICodecAPI> codec_api_;
  ComPtr<EventPump> pump_;
  DWORD input_id_ = 0;
  DWORD output_id_ = 0;
  bool async_ = false;
  bool streaming_ = false;
  bool provides_samples_ = false;
  DWORD output_size_ = 0;
  int need_input_ = 0;
  int stalls_ = 0;
  bool failed_ = false;
  bool sending_ = false;
  bool key_pending_ = true;
  int awaiting_key_ = 0;
  int width_ = 0;
  int height_ = 0;
  uint32_t fps_ = 30;
  uint32_t bps_ = 1000000;
  LONGLONG last_time_ = -1;
  std::deque<Pending> pending_;
  std::vector<uint8_t> sequence_header_;
  webrtc::H264BitstreamParser parser_;
  std::string name_;
  std::string error_;
  size_t delivered_ = 0;
  size_t keyframes_ = 0;
  size_t bytes_ = 0;
};

struct Probe {
  bool supported = false;
  std::string name;
};

// The first hardware MFT that opens and takes a 720p H.264 output type.
const Probe& ProbeHardware() {
  static const Probe probe = []() {
    Probe found;
    EnsureCom();
    MfStarted started;
    if (!started.ok())
      return found;
    for (auto& activate : EncoderMfts(true)) {
      std::string name = FriendlyName(activate.Get());
      ComPtr<IMFTransform> transform;
      HRESULT hr = activate->ActivateObject(IID_PPV_ARGS(&transform));
      if (FAILED(hr)) {
        RTC_LOG(LS_INFO) << "Media Foundation: " << name
                         << " does not open: " << Hex(hr);
        continue;
      }
      ComPtr<IMFAttributes> attributes;
      UINT32 async = 0;
      if (SUCCEEDED(transform->GetAttributes(&attributes)) &&
          SUCCEEDED(attributes->GetUINT32(MF_TRANSFORM_ASYNC, &async)) &&
          async) {
        attributes->SetUINT32(MF_TRANSFORM_ASYNC_UNLOCK, TRUE);
      }
      DWORD input_id = 0, output_id = 0;
      if (transform->GetStreamIDs(1, &input_id, 1, &output_id) != S_OK) {
        input_id = 0;
        output_id = 0;
      }
      ComPtr<IMFMediaType> type;
      hr = Mf()->create_media_type(&type);
      if (SUCCEEDED(hr)) {
        type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
        type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
        type->SetUINT32(MF_MT_AVG_BITRATE, 2000000);
        MFSetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, 1280, 720);
        MFSetAttributeRatio(type.Get(), MF_MT_FRAME_RATE, 30, 1);
        type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
        hr = transform->SetOutputType(output_id, type.Get(), 0);
      }
      ComPtr<IMFShutdown> shutdown;
      if (SUCCEEDED(transform.As(&shutdown)))
        shutdown->Shutdown();
      transform.Reset();
      activate->ShutdownObject();
      if (FAILED(hr)) {
        RTC_LOG(LS_INFO) << "Media Foundation: " << name
                         << " refuses H.264 720p: " << Hex(hr);
        continue;
      }
      found.supported = true;
      found.name = name;
      RTC_LOG(LS_INFO) << "Media Foundation hardware H.264 encoder: " << name;
      break;
    }
    return found;
  }();
  return probe;
}

}  // namespace

MediaFoundationVideoEncoderFactory::MediaFoundationVideoEncoderFactory() {
  formats_.push_back(webrtc::SdpVideoFormat(
      "H264", {{"profile-level-id", "42e01f"},
               {"level-asymmetry-allowed", "1"},
               {"packetization-mode", "1"}}));
}

bool MediaFoundationVideoEncoderFactory::IsSupported() {
  return ProbeHardware().supported;
}

std::string MediaFoundationVideoEncoderFactory::EncoderName() {
  return ProbeHardware().name;
}

std::vector<webrtc::SdpVideoFormat>
MediaFoundationVideoEncoderFactory::GetSupportedFormats() const {
  return formats_;
}

std::vector<webrtc::SdpVideoFormat>
MediaFoundationVideoEncoderFactory::GetImplementations() const {
  return formats_;
}

std::unique_ptr<webrtc::VideoEncoder> MediaFoundationVideoEncoderFactory::Create(
    const webrtc::Environment& env,
    const webrtc::SdpVideoFormat& format) {
  bool supported = false;
  for (const auto& f : formats_)
    supported = supported || format.IsSameCodec(f);
  if (!supported)
    return nullptr;
  // OpenH264 takes over if the MFT does not open or stops working.
  return webrtc::CreateVideoEncoderSoftwareFallbackWrapper(
      env,
      webrtc::CreateH264Encoder(env, webrtc::H264EncoderSettings::Parse(format)),
      std::make_unique<MediaFoundationH264Encoder>(format, false),
      false);
}

namespace {
// Keeps what the encoder gives (the self-tests).
class KeepingCallback : public webrtc::EncodedImageCallback {
 public:
  explicit KeepingCallback(std::vector<webrtc::EncodedImage>* out)
      : out_(out) {}
  Result OnEncodedImage(const webrtc::EncodedImage& image,
                        const webrtc::CodecSpecificInfo*) override {
    if (out_)
      out_->push_back(image);
    return Result(Result::OK);
  }
  void OnFrameDropped(uint32_t, int, bool) override {}

 private:
  std::vector<webrtc::EncodedImage>* out_;
};
}  // namespace

uint8_t MediaFoundationTestLuma(int x, int y, int picture) {
  return static_cast<uint8_t>(x + y + picture * 4);
}

uint32_t MediaFoundationTestRtpTimestamp(int picture) {
  return 1000 + picture * 3000;
}

std::string MediaFoundationH264Encode(bool software,
                                      int width,
                                      int height,
                                      int frames,
                                      std::vector<webrtc::EncodedImage>* out) {
  EnsureCom();
  if (!Mf())
    return "unavailable: no mfplat.dll (Media Foundation)";
  if (!software && !ProbeHardware().supported)
    return "unavailable: no hardware H.264 encoder MFT";
  webrtc::SdpVideoFormat format("H264", {{"profile-level-id", "42e01f"},
                                         {"level-asymmetry-allowed", "1"},
                                         {"packetization-mode", "1"}});
  MediaFoundationH264Encoder encoder(format, software);
  KeepingCallback callback(out);
  encoder.RegisterEncodeCompleteCallback(&callback);
  webrtc::VideoCodec codec;
  codec.codecType = webrtc::kVideoCodecH264;
  codec.width = width;
  codec.height = height;
  codec.maxFramerate = 30;
  codec.startBitrate = 800;
  codec.maxBitrate = 1500;
  codec.minBitrate = 100;
  webrtc::VideoEncoder::Settings settings(
      webrtc::VideoEncoder::Capabilities(false), 1, 1200);
  int32_t init = encoder.InitEncode(&codec, settings);
  if (init != WEBRTC_VIDEO_CODEC_OK) {
    std::string error = encoder.error();
    if (software && error.rfind("no software", 0) == 0)
      return "unavailable: " + error;
    return "error: InitEncode " + std::to_string(init) + ": " + error;
  }
  for (int i = 0; i < frames; ++i) {
    auto buffer = webrtc::I420Buffer::Create(width, height);
    // A moving gradient, so pictures differ.
    for (int y = 0; y < height; ++y)
      for (int x = 0; x < width; ++x)
        buffer->MutableDataY()[y * buffer->StrideY() + x] =
            MediaFoundationTestLuma(x, y, i);
    memset(buffer->MutableDataU(), 128,
           buffer->StrideU() * buffer->ChromaHeight());
    memset(buffer->MutableDataV(), 128,
           buffer->StrideV() * buffer->ChromaHeight());
    webrtc::VideoFrame frame =
        webrtc::VideoFrame::Builder()
            .set_video_frame_buffer(buffer)
            .set_rtp_timestamp(MediaFoundationTestRtpTimestamp(i))
            .set_timestamp_us(1000000 + i * 33333)
            .build();
    std::vector<webrtc::VideoFrameType> types = {
        i == 0 ? webrtc::VideoFrameType::kVideoFrameKey
               : webrtc::VideoFrameType::kVideoFrameDelta};
    int32_t r = encoder.Encode(frame, &types);
    if (r != WEBRTC_VIDEO_CODEC_OK)
      return "error: " + encoder.name() + ": Encode " + std::to_string(r) +
             " at picture " + std::to_string(i) + ": " + encoder.error();
  }
  encoder.Flush(1000);
  std::string report = encoder.name() + ": " + std::to_string(frames) +
                       " pictures in, " + std::to_string(encoder.delivered()) +
                       " out, " + std::to_string(encoder.keyframes()) +
                       " key, " + std::to_string(encoder.bytes()) + " bytes";
  encoder.Release();
  // Allow for a few still inside an encoder that holds some back.
  if (encoder.delivered() + 8 < static_cast<size_t>(frames) ||
      encoder.keyframes() == 0)
    return "error: " + report;
  return "ok: " + report;
}

std::string MediaFoundationH264SelfTest(bool software, int frames) {
  return MediaFoundationH264Encode(software, 640, 360, frames, nullptr);
}

}  // namespace livekit_ffi
