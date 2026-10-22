# IIDX 34 / 010-0916: audio-offset and file-integrity investigation

Date: 2026-10-20. This is a read-only follow-up to the
[adapter analysis](iidx34-analysis.md), prompted by concern that the supplied
`bm2dx.dll` might contain an added audio offset.

Target: `C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll`

SHA-256: `3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83`.
No changes were made to this file.

## Conclusion and limits

No evidence of an added third-party audio-offset patch was found in the inspected
paths. More strongly, the executable's Authenticode-covered bytes match the
digest in its embedded signature, and that CMS signature verifies with its
included certificate. This supports the file being intact as signed, rather
than subsequently patched on disk.

The included certificate identifies Konami's e-AMUSEMENT license signer.
Windows cannot build its chain to a trusted root on this machine, so this is
**not independent authentication of Konami provenance**. No separately obtained,
known-clean copy of this exact build was available for a byte-for-byte comparison.
The file has native synchronization logic; intact signing does not mean that all
game timing corrections are zero. Runtime patches, external plugins, driver
latency and active configuration were not inspected.

## Signature evidence

| Check | Observed result |
|---|---|
| Recomputed PE Authenticode SHA-1 | `776021351485710542b9d73e9826f1a72b2a230b` |
| Digest embedded in signed `SpcIndirectDataContent` | Exact match |
| CMS `CheckSignature(true)` | Passed; certificate-chain trust deliberately not implied |
| Certificate subject | `CN=license signer, OU=license signer, OU=e-AMUSEMENT, O="Konami Digital Entertainment Co., Ltd.", C=JP` |
| Certificate thumbprint | `526C7F322F95CBB1556044C1FCAF449D787C1EB2` |
| Windows Authenticode status | `UnknownError`: certificate chain could not be built to a trusted root |
| Certificate table | File offset `0x115b000`, 1688 bytes, ends at EOF |
| PE timestamp | 2026-09-10 05:17:11 UTC; not proof of the distribution/release date |

The image has contiguous raw PE sections. The checksum field, security-directory
entry and certificate table are excluded from the Authenticode digest, as required
by that format; executable code and normal data are covered. This check therefore
does not claim every byte is covered, or that a trusted-root verification passed.

Reproduce without loading or executing the game:

```powershell
./scripts/inspect-iidx34-signature.ps1 -Dll 'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll'
```

The script checks the observed PE layout before hashing, validates the signed
content's SHA-1 digest structure, verifies CMS mathematics, compares the file
digest, and separately reports Windows trust status. It rejects unsupported
layouts rather than acting as a general-purpose Authenticode verifier.

## Audio/timing paths inspected

All addresses below are RVAs in the target.

| RVA | Finding |
|---|---|
| `0x811000` | Sequencer tick: compares the sound position with chart time; contains the same type of native synchronization logic documented for 33 |
| `0x5b8090` → `0xa7e5a0` | Sound-position wrapper and backend dispatch; result is integer conversion of backend position +0.1, not an identified custom millisecond offset |
| `0x593930` / `0x593940` | Cached-byte getters used by synchronization branches; read `0xb0c5014` / `0xb0c5015`, rather than forced-ON/OFF patched immediates |
| `0xac2a30` | Outgoing music-play serializer, including native unsynchronization telemetry |

In `0x811000`, the sound-leading threshold remains 17 ms and the
chart-leading threshold 34 ms. A correction branch accumulates one frame or
the measured difference, depending on the native flag. The end-of-playback
framerate correction has a 17 ms error threshold and accepted range
118.5 ≤ FPS < 121.5. These are synchronization thresholds/calibration,
**not evidence of a constant +17 ms or +34 ms audio delay**.

This was a targeted integrity/timing review, not an exhaustive proof about every
audio backend. In particular, no claim is made that native backend latency
compensation is absent or that runtime timing matches another installation.

## False leads resolved

- The string `` `omni callsig' `` at `0xe0cc28` occurs in a compiler
  symbol-name table alongside vector-constructor, RTTI and related names. It
  also exists in the earlier 33 binary. It is not evidence of Omnifix.
- `latency@` at `0xdfb8e0` is referenced by `0xac2a30` when serializing
  `music_play_log/omission_log`; the string is not an audio-offset setting.
- `g_offset` strings belong to shader code; other `sample_offset` /
  `compositionOffset` strings belong to media-container handling.
- The 1688 bytes after the PE sections are the embedded certificate table,
  not unexplained appended patch code.

The `bilimani_iidx.dll` built for adapter support does not add an audio-offset
feature. Its playback code reads the native chart clock for display; it does not
modify that clock or hook an audio backend.
