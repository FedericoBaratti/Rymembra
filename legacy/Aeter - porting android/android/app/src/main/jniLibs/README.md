# jniLibs — prebuilt native libraries (android-arm64)

These `.so` files are **not** committed (see `.gitignore`); add them during setup.
On Android 14+/16, executables may only run from `nativeLibraryDir`, which is
populated from this folder — hence the tool binaries are shipped as `lib*.so`.

Place files under `arm64-v8a/` (add `armeabi-v7a/`/`x86_64/` only if you target
those ABIs):

```
arm64-v8a/
  libnode.so          # nodejs-mobile prebuilt engine
  libnode-bridge.so   # JNI glue exposing startNodeWithArguments / channel
  libytdlp.so         # yt-dlp ARM build (renamed; see binaries.ts mapping)
  libffmpeg.so        # ffmpeg ARM build
  libfpcalc.so        # chromaprint/fpcalc ARM build
```

Binary name mapping is in `electron/modules/binaries.ts` (`lib<name>.so`, dashes
stripped → `yt-dlp` becomes `libytdlp.so`). `spotdl` is intentionally absent (no
practical ARM build); Spotify links degrade to yt-dlp search. See `MOBILE.md`.
