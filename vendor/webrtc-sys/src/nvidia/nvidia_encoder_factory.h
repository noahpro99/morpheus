

#ifndef NVIDIA_VIDEO_ENCODER_FACTORY_H_
#define NVIDIA_VIDEO_ENCODER_FACTORY_H_

#include <mutex>
#include <vector>

#include "api/environment/environment.h"
#include "api/video_codecs/sdp_video_format.h"
#include "api/video_codecs/video_encoder_factory.h"
#include "cuda_context.h"

namespace webrtc {

class NvidiaVideoEncoderFactory : public VideoEncoderFactory {
 public:
  NvidiaVideoEncoderFactory();
  ~NvidiaVideoEncoderFactory() override;

  // Whether NVENC works: opens a CUDA context and an encode session once.
  static bool IsSupported();
  // Whether it may: no CUDA started, nothing loaded.
  static bool MightBeSupported();

  std::unique_ptr<VideoEncoder> Create(const Environment& env,
                                       const SdpVideoFormat& format) override;

  // Returns a list of supported codecs in order of preference.
  std::vector<SdpVideoFormat> GetSupportedFormats() const override;

  std::vector<SdpVideoFormat> GetImplementations() const override;

  std::unique_ptr<EncoderSelectorInterface> GetEncoderSelector()
      const override {
    return nullptr;
  }

 private:
  static bool IsAv1Supported();
  void EnsureFormats() const;

  mutable std::mutex mutex_;
  mutable bool formats_ready_ = false;
  mutable std::vector<SdpVideoFormat> supported_formats_;
  livekit_ffi::CudaContext* cu_context_ = nullptr;
};

}  // namespace webrtc

#endif  // NVIDIA_VIDEO_ENCODER_FACTORY_H_
