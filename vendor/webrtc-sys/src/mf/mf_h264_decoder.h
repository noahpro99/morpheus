/*
 * Morpheus patch: hardware H.264 decoding on Windows through Media
 * Foundation: Microsoft's H.264 decoder MFT given a Direct3D 11 device, so
 * it decodes on the GPU (DXVA: Intel Quick Sync, AMD, NVIDIA), with the
 * pictures copied back to I420 for libwebrtc. mfplat.dll is loaded at run
 * time; no GPU decoder, or one that fails, means FFmpeg in software through
 * libwebrtc's fallback wrapper.
 */

#pragma once

#include <memory>
#include <string>
#include <vector>

#include "api/environment/environment.h"
#include "api/video_codecs/sdp_video_format.h"
#include "api/video_codecs/video_decoder.h"
#include "api/video_codecs/video_decoder_factory.h"

namespace livekit_ffi {

class MediaFoundationVideoDecoderFactory : public webrtc::VideoDecoderFactory {
 public:
  // `software_h264`: whether libwebrtc's own H.264 decoder (FFmpeg) works,
  // to fall back to.
  explicit MediaFoundationVideoDecoderFactory(bool software_h264);

  // Whether this computer decodes H.264 on its GPU through Media Foundation
  // (looked for once: a Direct3D 11 device with an H.264 decoder profile and
  // the decoder MFT opening on it).
  static bool IsSupported();
  // The GPU it decodes on ("Intel(R) UHD Graphics 620"), empty if none.
  static std::string DecoderName();
  // Why there is none, empty if there is one.
  static std::string Unavailable();

  std::vector<webrtc::SdpVideoFormat> GetSupportedFormats() const override;
  std::unique_ptr<webrtc::VideoDecoder> Create(
      const webrtc::Environment& env,
      const webrtc::SdpVideoFormat& format) override;

 private:
  const bool software_h264_;
  std::vector<webrtc::SdpVideoFormat> formats_;
};

// Encodes `frames` pictures of 640x360 with Microsoft's software H.264
// encoder MFT and decodes them with the Media Foundation decoder: on the GPU
// (`gpu`) or Microsoft's decoder in software (which CI machines have).
// "ok: ...", "unavailable: ..." or "error: ...".
std::string MediaFoundationH264DecodeSelfTest(bool gpu, int frames);

}  // namespace livekit_ffi
