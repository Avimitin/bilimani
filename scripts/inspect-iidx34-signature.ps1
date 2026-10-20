param([Parameter(Mandatory = $true)][string]$Dll)
# Read-only check for the analyzed IIDX 34 PE layout. Never loads the game.
# Does not establish certificate-chain trust or inspect runtime patches.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
$path = (Resolve-Path -LiteralPath $Dll).Path
$bytes = [IO.File]::ReadAllBytes($path)
function U16([int]$at) { [BitConverter]::ToUInt16($bytes, $at) }
function U32([int]$at) { [BitConverter]::ToUInt32($bytes, $at) }
function Hex([byte[]]$value) { ([BitConverter]::ToString($value)).Replace('-', '').ToLowerInvariant() }
function Der([byte[]]$value, [int]$at) {
    $tag = $value[$at]
    $size = [int]$value[$at + 1]
    $start = $at + 2
    if ($size -ge 128) {
        $count = $size -band 127
        if ($count -eq 0 -or $count -gt 4) { throw 'Unsupported DER length' }
        $size = 0
        for ($i = 0; $i -lt $count; $i++) { $size = ($size -shl 8) -bor $value[$start + $i] }
        $start += $count
    }
    if ($size -lt 0 -or $start + $size -gt $value.Length) { throw 'Truncated DER' }
    [pscustomobject]@{ Tag = $tag; Start = $start; Size = $size; End = $start + $size }
}

if ((U16 0) -ne 0x5a4d) { throw 'Not a PE file' }
$pe = U32 0x3c
if ((U32 $pe) -ne 0x4550 -or (U16 ($pe + 4)) -ne 0x8664) { throw 'Not an AMD64 PE' }
$optional = $pe + 24
if ((U16 $optional) -ne 0x20b) { throw 'Not PE32+' }
$checksum = $optional + 64
$security = $optional + 112 + 4 * 8
$certificate = U32 $security
$certificateSize = U32 ($security + 4)
if ($certificateSize -lt 8 -or $certificate + $certificateSize -ne $bytes.Length) {
    throw 'This checker requires the certificate table at EOF'
}
# Restrict the simple sequential hash to the observed contiguous raw layout.
$sections = $optional + (U16 ($pe + 20))
$ranges = for ($i = 0; $i -lt (U16 ($pe + 6)); $i++) {
    $at = $sections + $i * 40
    [pscustomobject]@{ Start = U32 ($at + 20); Size = U32 ($at + 16) }
}
$end = U32 ($optional + 60)
foreach ($range in ($ranges | Where-Object Size -gt 0 | Sort-Object Start)) {
    if ($range.Start -ne $end) { throw 'Noncontiguous section layout: unsupported by this checker' }
    $end += $range.Size
}
if ($end -ne $certificate) { throw 'Unexpected data before certificate' }
if ((U16 ($certificate + 4)) -ne 0x200 -or (U16 ($certificate + 6)) -ne 2) {
    throw 'Expected WIN_CERTIFICATE PKCS signed data'
}
$length = (U32 $certificate) - 8
if ($length -le 0 -or $length -gt $certificateSize - 8) { throw 'Invalid certificate length' }
$pkcs = New-Object byte[] $length
[Array]::Copy($bytes, $certificate + 8, $pkcs, 0, $length)
$cms = New-Object Security.Cryptography.Pkcs.SignedCms
$cms.Decode($pkcs)
if ($cms.ContentInfo.ContentType.Value -ne '1.3.6.1.4.1.311.2.1.4') {
    throw 'Expected Authenticode SpcIndirectDataContent'
}
$cms.CheckSignature($true) # Cryptographic signature only; do not trust the root implicitly.
$content = $cms.ContentInfo.Content
$outer = Der $content 0
$data = Der $content $outer.Start
$digestInfo = Der $content $data.End
$algorithm = Der $content $digestInfo.Start
$oid = Der $content $algorithm.Start
$digest = Der $content $algorithm.End
if ($outer.Tag -ne 0x30 -or $digestInfo.Tag -ne 0x30 -or $oid.Tag -ne 6 -or
    (Hex $content[$oid.Start..($oid.End - 1)]) -ne '2b0e03021a' -or
    $digest.Tag -ne 4 -or $digest.Size -ne 20 -or $digest.End -ne $content.Length) {
    throw 'Expected SHA-1 Authenticode digest structure'
}
$expected = Hex $content[$digest.Start..($digest.End - 1)]
$sha = [Security.Cryptography.SHA1]::Create()
try {
    # Authenticode excludes CheckSum, security-directory entry, and certificate table.
    [void]$sha.TransformBlock($bytes, 0, $checksum, $bytes, 0)
    [void]$sha.TransformBlock($bytes, $checksum + 4, $security - $checksum - 4, $bytes, $checksum + 4)
    [void]$sha.TransformBlock($bytes, $security + 8, $certificate - $security - 8, $bytes, $security + 8)
    [void]$sha.TransformFinalBlock([byte[]]@(), 0, 0)
    $actual = Hex $sha.Hash
} finally { $sha.Dispose() }
if ($actual -ne $expected) { throw "Authenticode digest mismatch: expected $expected, got $actual" }
$windows = Get-AuthenticodeSignature -LiteralPath $path
[pscustomobject]@{
    FileSha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    AuthenticodeSha1 = $actual
    SignedDigestMatches = $true
    CmsSignatureVerified = $true
    Signer = $cms.SignerInfos[0].Certificate.Subject
    SignerThumbprint = $cms.SignerInfos[0].Certificate.Thumbprint
    WindowsTrustStatus = $windows.Status.ToString()
    WindowsTrustMessage = $windows.StatusMessage
} | ConvertTo-Json
