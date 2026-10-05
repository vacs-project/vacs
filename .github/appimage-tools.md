# AppImage bundler tools

tauri-bundler downloads these tools into its cache for the AppImage bundle, without a checksum, and
runs them in the release build next to the signing key. It skips any download whose file is
already cached, so the release job seeds the cache with the copies below and fails on a hash
mismatch instead of shipping a changed upstream file under our signature.

CI compares this table with what the locked `@tauri-apps/cli` would download
(`.github/scripts/appimage_tools.py check-bundler`). Review a new upstream file before changing
its hash.

| Cache file | SHA-256 | Source | Bundler URL, if different |
| --- | --- | --- | --- |
| `AppRun-x86_64` | `f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f` | https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64 | |
| `linuxdeploy-07333c6-x86_64.AppImage` | `36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62` | https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage | |
| `linuxdeploy-plugin-appimage.AppImage` | `992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3` | https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage | https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage |

`linuxdeploy-plugin-appimage` comes from a fixed release because `continuous` is rebuilt from the
same source every month, changing its hash each time.
