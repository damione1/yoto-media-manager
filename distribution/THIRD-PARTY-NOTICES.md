Yoto's own code has no licence selected by its author yet. This notice does not assign it GPL or any other licence.

The application invokes separately distributed programs; it does not link their GPL code into the GUI. Their redistribution conditions remain applicable to those programs.

FFmpeg on macOS is built from the included 9.0.2 source tarball with shared libraries, GPL disabled and no external autodetection. FFmpeg LGPL texts, exact configure/build instructions and the LAME 3.100 source and applied symbol-export change are included. LAME is LGPL-2.0-or-later. Windows uses the pinned BtbN 9.0.2+17 LGPLv3 shared build with DLLs and its licence. Corresponding FFmpeg/BtbN build sources: https://github.com/BtbN/FFmpeg-Builds/tree/autobuild-2026-09-30-13-08 and https://ffmpeg.org/releases/. Recipients may replace the sidecar libraries in their own copy; modified macOS bundles will need their own code signature.

The combined yt-dlp PyInstaller executables are GPLv3-or-later, including their Python and other bundled components; yt-dlp's Python sources alone are Unlicense. Upstream licences, dependency notices and corresponding source/build recipe: https://github.com/yt-dlp/yt-dlp/tree/2026.08.19 and https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/THIRD-PARTY-LICENSES.txt. The pinned release executables and corresponding source archive accompany the distribution. They must not be represented as Unlicense-only.

Deno is MIT-licensed, with third-party notices: https://github.com/denoland/deno/tree/v2.9.7. Upstream source and licence archive accompany the distribution. All sidecar upstream URLs and SHA256 values are pinned in sidecars.json; manifest.json records the final distributed files after preparing them. Code signing changes executable bytes, and the bundle manifest is regenerated after signing.

Windows FFmpeg corresponding source is pinned to commit `2a571b606854520cf89804d8030c8b328e621689`, with the BtbN build recipes at `6c9aec5fc9a72ec3abedd1fa84db141fa18cf52b`. Both checksummed archives accompany the installer in `tools/sources`; this is the actual 9.0.2+17 build, beyond the macOS 9.0.2 release source.
