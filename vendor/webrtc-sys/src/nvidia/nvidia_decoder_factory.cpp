#include "nvidia_decoder_factory.h"
#include "../driver_dlopen.h"  // Morpheus patch

#include <modules/video_coding/codecs/h264/include/h264.h>

#include <cstdlib>
#include <dlfcn.h>
#include <memory>
#include <mutex>

#include "cuda_context.h"
#include "h264_decoder_impl.h"
#include "h265_decoder_impl.h"
#include "rtc_base/logging.h"

namespace webrtc {

namespace {

constexpr char kDisableNvdecEnvVar[] = "LK_DISABLE_NVDEC";

bool IsNvdecDisabledByEnv() {
  return std::getenv(kDisableNvdecEnvVar) != nullptr;
}

void LogNvdecDisabledByEnv() {
  static std::once_flag log_once;
  std::call_once(log_once, [] {
    RTC_LOG(LS_INFO) << "NVIDIA NVDEC disabled because " << kDisableNvdecEnvVar
                     << " is set.";
  });
}

}  // namespace

constexpr char kSdpKeyNameCodecImpl[] = "implementation_name";
constexpr char kCodecName[] = "NvCodec";
constexpr char kNvdecRuntimeLibrary[] = "libnvcuvid.so.1";

namespace {

bool IsNvdecRuntimeAvailable() {
  void* hModule = lk_driver_dlopen(kNvdecRuntimeLibrary, RTLD_LAZY | RTLD_LOCAL);
  if (!hModule) {
    RTC_LOG(LS_WARNING) << "NVDEC runtime library (" << kNvdecRuntimeLibrary
                        << ") not found, hardware decoding unavailable.";
    return false;
  }

  dlclose(hModule);
  return true;
}

}  // namespace

std::vector<SdpVideoFormat> SupportedNvDecoderCodecsForGeneration(int major) {
  std::vector<SdpVideoFormat> supportedFormats;

  // HardwareGeneration Kepler is 3.x
  // https://docs.nvidia.com/deploy/cuda-compatibility/index.html#faq
  // Kepler support h264 profile Main, Highprofile up to Level4.1
  // https://docs.nvidia.com/video-technologies/video-codec-sdk/nvdec-video-decoder-api-prog-guide/index.html#video-decoder-capabilities__table_o3x_fms_3lb
  if (major <= 3) {
    supportedFormats = {
        CreateH264Format(webrtc::H264Profile::kProfileHigh,
                         webrtc::H264Level::kLevel4_1, "1"),
        CreateH264Format(webrtc::H264Profile::kProfileMain,
                         webrtc::H264Level::kLevel4_1, "1"),
    };
  } else {
    supportedFormats = {
        // Constrained Baseline Profile does not support NvDecoder, but WebRTC
        // uses this profile by default,
        // so it must be returned in this method.
        CreateH264Format(webrtc::H264Profile::kProfileConstrainedBaseline,
                         webrtc::H264Level::kLevel5_1, "1"),
        CreateH264Format(webrtc::H264Profile::kProfileBaseline,
                         webrtc::H264Level::kLevel5_1, "1"),
        CreateH264Format(webrtc::H264Profile::kProfileHigh,
                         webrtc::H264Level::kLevel5_1, "1"),
        CreateH264Format(webrtc::H264Profile::kProfileMain,
                         webrtc::H264Level::kLevel5_1, "1"),
        SdpVideoFormat("H265"),
        SdpVideoFormat("HEVC"),
    };
  }

  for (auto& format : supportedFormats) {
    format.parameters.emplace(kSdpKeyNameCodecImpl, kCodecName);
  }

  return supportedFormats;
}

// Morpheus patch (#10): nothing of CUDA is started here. Starting the driver
// (cuInit, a context) costs 100-200 MB of resident memory, and a call that
// receives no hardware-decodable video never needs it. The driver is started
// when the first decoder is wanted (Create), the context with it.
NvidiaVideoDecoderFactory::NvidiaVideoDecoderFactory()
    : cu_context_(nullptr) {}

void NvidiaVideoDecoderFactory::EnsureFormats() const {
  std::lock_guard<std::mutex> lock(mutex_);
  if (formats_ready_) {
    return;
  }
  formats_ready_ = true;
  if (IsNvdecDisabledByEnv()) {
    LogNvdecDisabledByEnv();
    return;
  }
  if (!livekit_ffi::CudaContext::IsAvailable() || !IsNvdecRuntimeAvailable()) {
    RTC_LOG(LS_INFO) << "NvidiaVideoDecoderFactory: no NVDEC support.";
    return;
  }
  // The generation of the GPU decides the formats; asking needs the driver
  // started but no context.
  int major = 0;
  CUdevice device;
  if (cuDeviceGet(&device, 0) == CUDA_SUCCESS) {
    cuDeviceGetAttribute(&major, CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
                         device);
  }
  supported_formats_ = SupportedNvDecoderCodecsForGeneration(major);
  RTC_LOG(LS_INFO) << "NvidiaVideoDecoderFactory created with "
                   << supported_formats_.size() << " supported formats.";
}

NvidiaVideoDecoderFactory::~NvidiaVideoDecoderFactory() {
  if (cu_context_) {
    // This ownership model relies on WebRTC releasing decoders before their
    // owning factory, so the factory lifetime bounds this context reference.
    RTC_LOG(LS_INFO) << "NvidiaVideoDecoderFactory destroyed; releasing CUDA "
                        "context.";
    cu_context_->Shutdown();
    cu_context_ = nullptr;
  }
}

bool NvidiaVideoDecoderFactory::IsSupported() {
  if (IsNvdecDisabledByEnv()) {
    LogNvdecDisabledByEnv();
    return false;
  }

  // Morpheus patch: only what costs nothing to find out: the driver's device
  // node. CUDA itself starts when a decoder is
  // wanted.
  return livekit_ffi::CudaContext::DriverPresent();
}

std::unique_ptr<VideoDecoder> NvidiaVideoDecoderFactory::Create(
    const Environment& env,
    const SdpVideoFormat& format) {
  EnsureFormats();
  // Check if the requested format is supported.
  for (const auto& supported_format : supported_formats_) {
    if (format.IsSameCodec(supported_format)) {
      // If the format is supported, create and return the decoder.
      if (!cu_context_) {
        auto* ctx = livekit_ffi::CudaContext::GetInstance();
        if (!ctx->Initialize()) {
          RTC_LOG(LS_ERROR) << "Failed to initialize CUDA context.";
          return nullptr;
        }
        cu_context_ = ctx;
      }
      if (format.name == "H264") {
        RTC_LOG(LS_INFO) << "Using NVIDIA HW decoder (NVDEC) for H264";
        return std::make_unique<NvidiaH264DecoderImpl>(cu_context_->GetContext());
      }
      if (format.name == "H265" || format.name == "HEVC") {
        RTC_LOG(LS_INFO) << "Using NVIDIA HW decoder (NVDEC) for H265/HEVC";
        return std::make_unique<NvidiaH265DecoderImpl>(cu_context_->GetContext());
      }
    }
  }
  return nullptr;
}

std::vector<SdpVideoFormat> NvidiaVideoDecoderFactory::GetSupportedFormats()
    const {
  // WebRTC asks when a connection is made, in a voice-only call too. Until a
  // decoder is wanted the answer is the H.264 a GPU of this century decodes,
  // worked out without starting CUDA or loading a library (the driver's device
  // node). Create() then finds out for real, and a GPU that cannot is passed
  // over for the software decoder (which lists the same H.264).
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (formats_ready_) {
      return supported_formats_;
    }
  }
  if (IsNvdecDisabledByEnv() || !livekit_ffi::CudaContext::DriverPresent()) {
    return {};
  }
  std::vector<SdpVideoFormat> formats;
  for (auto& format : SupportedNvDecoderCodecsForGeneration(4)) {
    if (format.name == "H264") {
      formats.push_back(format);
    }
  }
  return formats;
}

}  // namespace webrtc
