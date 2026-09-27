# References and third-party notices

The Bilibili transport is a Rust adaptation of the protocol/signing/session logic in:

- [xfgryujk/blivedm](https://github.com/xfgryujk/blivedm), commit
  `3bf17fe862b8c2fc6bc04c81c3f0c5db95de93d6`, MIT, copyright (c) 2018 xfgryujk.
- [xfgryujk/blivechat](https://github.com/xfgryujk/blivechat), commit
  `848409df78d2cca65d3cc8ffe938eb6b6f261b76`, MIT, copyright (c) 2019 xfgryujk.

[aixxe/playlister5](https://github.com/aixxe/playlister5), commit
`24b9d5939e614de98fbc81fbbf8e99b6fb124b21`, MIT, copyright 2026 aixxe,
was used as a reference for game terminology, difficulty ordering and analysis
starting points. It is not a runtime dependency.

[aixxe/2dxtra](https://github.com/aixxe/2dxtra), commit
`a6fc091a0914f578498ca297bf30dca0ffa1bfb5`, MIT, copyright 2025 aixxe,
was used as a reference for controller menu navigation and the IIDX input layout.
The input hook was independently checked against the supported game binary in IDA.
It is not a runtime dependency.

The MIT permission and warranty terms for these upstream projects follow:

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Spice loading order and SDK ABI were inspected in
[spice2x](https://github.com/spice2x/spice2x.github.io), commit
`7aba7da3b67f3e4c8b0e950f306bd044d69fa494`. No Spice implementation is incorporated.
The game ABI was independently checked against the locally supplied binary in IDA.

Rust crate versions are pinned in `Cargo.lock`; see each crate's accompanying
license. Release packaging collects dependency license files under `licenses/`.
No proprietary game code, data or IDA database is distributed.

Configuration persistence uses [rusqlite](https://github.com/rusqlite/rusqlite),
MIT, with bundled [SQLite](https://sqlite.org/), public domain. The database engine
is linked into the DLL; users do not need a separate SQLite installation.

The in-game panel uses [egui](https://github.com/emilk/egui), MIT OR Apache-2.0.
Its components and design tokens use
[ouroboros-ui](https://github.com/Type-zero-labs/ouroboros-ui), commit
`c390d7deffa7955e28b2e3bcb9c22ac0899a261b`, MIT, copyright 2026 Type Zero Labs.
Bundled Iosevka fonts are licensed under SIL Open Font License 1.1; Phosphor
icons are provided by egui-phosphor under MIT. Release packages include the
upstream code license, font license and credits under `licenses/`.
Its D3D9 painter and Win32 input bridge are implemented in this project; no
egui-d3d9 implementation is incorporated. Spice SDK v0.4 drawing ABI is declared
by the upstream `sdk/include/spicesdk.h`. Chinese/Japanese fonts are read from
the user's Windows installation and are not included in the DLL or ZIP.
