/*
 * Morpheus patch: dlopen a GPU driver library (libva, libcuda, libnvcuvid,
 * libnvidia-encode) by soname, and where that fails from the directories
 * distributions keep them in without putting them on the loader's path:
 * NixOS's /run/opengl-driver/lib (nothing there is on the default search
 * path of a binary not built by Nix). Plain C, so the implib lazy-load
 * stubs can use it too.
 */
#ifndef LIVEKIT_DRIVER_DLOPEN_H
#define LIVEKIT_DRIVER_DLOPEN_H

#if defined(__linux__)
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

static inline void* lk_driver_dlopen(const char* soname, int flags) {
  void* handle = dlopen(soname, flags);
  if (handle || strchr(soname, '/')) {
    return handle;
  }
  static const char* const dirs[] = {
      "/run/opengl-driver/lib",
  };
  for (size_t i = 0; i < sizeof(dirs) / sizeof(dirs[0]); i++) {
    char path[512];
    if (snprintf(path, sizeof(path), "%s/%s", dirs[i], soname) >=
        (int)sizeof(path)) {
      continue;
    }
    handle = dlopen(path, flags);
    if (handle) {
      return handle;
    }
  }
  return NULL;
}
#endif

#endif  // LIVEKIT_DRIVER_DLOPEN_H
