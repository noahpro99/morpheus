/*
 * Morpheus patch: hardware H.264 encoding on Windows through Media
 * Foundation (the GPU's encoder MFT: Intel Quick Sync, AMD AMF, NVIDIA
 * NVENC all ship one with their drivers). mfplat.dll is loaded at run time,
 * so a Windows without Media Foundation (N editions) still starts; no
 * hardware MFT, or one that fails, means OpenH264 in software.
 */

#pragma once

#include <memory>
#include <string>
#include <vector>

#include "api/environment/environment.h"
#include "api/video/encoded_image.h"
#include "api/video_codecs/sdp_video_format.h"
#include "api/video_codecs/video_encoder.h"
#include "api/video_codecs/video_encoder_factory.h"

namespace livekit_ffi {

class MediaFoundationVideoEncoderFactory : public webrtc::VideoEncoderFactory {
 public:
  MediaFoundationVideoEncoderFactory();

  // Whether this computer has a hardware H.264 encoder MFT that opens
  // (looked for once).
  static bool IsSupported();
  // Its name ("Intel® Quick Sync Video H.264 Encoder MFT"), empty if none.
  static std::string EncoderName();

  std::vector<webrtc::SdpVideoFormat> GetSupportedFormats() const override;
  std::vector<webrtc::SdpVideoFormat> GetImplementations() const override;
  std::unique_ptr<webrtc::VideoEncoder> Create(
      const webrtc::Environment& env,
      const webrtc::SdpVideoFormat& format) override;

 private:
  std::vector<webrtc::SdpVideoFormat> formats_;
};

// Encodes `frames` pictures of 640x360 through the hardware MFT (or, with
// `software`, Microsoft's software one, which CI machines have) and says
// how it went: "ok: <name>: ..." or "unavailable: ..." or "error: ...".
std::string MediaFoundationH264SelfTest(bool software, int frames);

// The same with any size, keeping the encoded pictures in `out` (when not
// null) for the decoder's self-test. Picture `i` is a moving gradient: Y at
// (x, y) is MediaFoundationTestLuma(x, y, i), U and V 128, its RTP time
// MediaFoundationTestRtpTimestamp(i).
std::string MediaFoundationH264Encode(bool software,
                                      int width,
                                      int height,
                                      int frames,
                                      std::vector<webrtc::EncodedImage>* out);
uint8_t MediaFoundationTestLuma(int x, int y, int picture);
uint32_t MediaFoundationTestRtpTimestamp(int picture);

}  // namespace livekit_ffi
