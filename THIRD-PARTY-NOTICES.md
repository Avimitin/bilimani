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
