Yoto's own code has no licence selected by its author yet. This notice does not assign it GPL or any other licence.

The application invokes separately distributed programs; it does not link their GPL code into the GUI. Their redistribution conditions remain applicable to those programs.

FFmpeg on macOS is built from the included 9.0.2 source tarball with shared libraries, GPL disabled and no external autodetection. FFmpeg LGPL texts, exact configure/build instructions and the LAME 3.100 source and applied symbol-export change are included. LAME is LGPL-2.0-or-later. Windows uses the pinned BtbN 9.0.2+17 LGPLv3 shared build with DLLs and its licence. Corresponding FFmpeg/BtbN build sources: https://github.com/BtbN/FFmpeg-Builds/tree/autobuild-2026-09-30-13-08 and https://ffmpeg.org/releases/. Recipients may replace the sidecar libraries in their own copy; modified macOS bundles will need their own code signature.

The combined yt-dlp PyInstaller executables are GPLv3-or-later, including their Python and other bundled components; yt-dlp's Python sources alone are Unlicense. Upstream licences, dependency notices and corresponding source/build recipe: https://github.com/yt-dlp/yt-dlp/tree/2026.08.19 and https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/THIRD-PARTY-LICENSES.txt. The pinned release executables and corresponding source archive accompany the distribution. They must not be represented as Unlicense-only.

Deno is MIT-licensed, with third-party notices: https://github.com/denoland/deno/tree/v2.9.7. Upstream source and licence archive accompany the distribution. All sidecar upstream URLs and SHA256 values are pinned in sidecars.json; manifest.json records the final distributed files after preparing them. Code signing changes executable bytes, and the bundle manifest is regenerated after signing.

Windows FFmpeg corresponding source is pinned to commit `2a571b606854520cf89804d8030c8b328e621689`, with the BtbN build recipes at `6c9aec5fc9a72ec3abedd1fa84db141fa18cf52b`. Both checksummed archives accompany the installer in `tools/sources`; this is the actual 9.0.2+17 build, beyond the macOS 9.0.2 release source.

The desktop application embeds the Fredoka and Nunito typefaces (static SemiBold instance of Fredoka; Regular, SemiBold, Bold and ExtraBold instances of Nunito, as served by Google Fonts). Copyright 2016 The Fredoka Project Authors (https://github.com/hafontia/Fredoka-One). Copyright 2014 The Nunito Project Authors (https://github.com/googlefonts/nunito). Both are licensed under the SIL Open Font License, Version 1.1, reproduced below; the licence texts also accompany the font files in `assets/fonts`.

```text
This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at:
http://scripts.sil.org/OFL


-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded, 
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```
