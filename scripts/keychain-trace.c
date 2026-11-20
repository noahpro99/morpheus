// Logs every Security.framework keychain call of the process with its call
// stack (DYLD_INSERT_LIBRARIES; scripts/keychain-debug-check.sh, #159).
// clang -dynamiclib -o keychain-trace.dylib keychain-trace.c -framework Security -framework CoreFoundation
#include <Security/Security.h>
#include <execinfo.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define INTERPOSE(replacement, replacee) \
  __attribute__((used)) static struct { const void *r; const void *o; } \
  _interpose_##replacee __attribute__((section("__DATA,__interpose"))) = { \
    (const void *)(unsigned long)&replacement, (const void *)(unsigned long)&replacee }

static void note(const char *what) {
  const char *path = getenv("MORPHEUS_KEYCHAIN_TRACE_OUT");
  int fd = path ? open(path, O_WRONLY | O_APPEND | O_CREAT, 0644) : 2;
  if (fd < 0) return;
  char head[128];
  int n = snprintf(head, sizeof head, "KEYCHAIN CALL: %s\n", what);
  write(fd, head, n);
  void *frames[40];
  int count = backtrace(frames, 40);
  backtrace_symbols_fd(frames, count, fd);
  write(fd, "\n", 1);
  if (path) close(fd);
}

#define WRAP(ret, name, params, args) \
  static ret my_##name params { note(#name); return name args; } \
  INTERPOSE(my_##name, name)

WRAP(OSStatus, SecItemCopyMatching, (CFDictionaryRef q, CFTypeRef *r), (q, r));
WRAP(OSStatus, SecItemAdd, (CFDictionaryRef a, CFTypeRef *r), (a, r));
WRAP(OSStatus, SecItemUpdate, (CFDictionaryRef q, CFDictionaryRef u), (q, u));
WRAP(OSStatus, SecItemDelete, (CFDictionaryRef q), (q));
