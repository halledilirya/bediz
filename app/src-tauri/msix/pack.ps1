param(
    [string]$StoreName,
    [string]$StorePublisher,
    [string]$StorePublisherDisplayName
)

$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$app = Join-Path $root "app"
$exe = Join-Path $root "target\release\bediz.exe"
$out = Join-Path $root "target\msix"
$stage = Join-Path $out "Bediz"
$icons = Join-Path $out "icons"

function Invoke-Tool([string]$Tool, [string[]]$Arguments) {
    & $Tool @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Tool exited with $LASTEXITCODE" }
}

function Find-SdkTool([string]$File) {
    $found = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\$File" -ErrorAction SilentlyContinue |
        Sort-Object { [version]$_.Directory.Parent.Name } |
        Select-Object -Last 1
    if (-not $found) { throw "$File was not found; install the Windows SDK" }
    $found.FullName
}

if (-not (Test-Path $exe)) { throw "Build the app first: npm run tauri build in the app folder" }
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
if ($version -notmatch '^\d{1,5}\.\d{1,5}\.\d{1,5}$' -or ($version.Split(".") | Where-Object { [int]$_ -gt 65535 })) {
    throw "An MSIX version is made of numbers up to 65535, but Cargo.toml says $version"
}
if ($StoreName -and $version.StartsWith("0.")) {
    throw "The Store refuses a package version that starts with 0, so Bediz goes there from 1.0.0 on; Cargo.toml says $version"
}

if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force (Join-Path $stage "Assets") | Out-Null
Copy-Item $exe $stage

$manifest = New-Object System.Xml.XmlDocument
$manifest.PreserveWhitespace = $true
$manifest.Load((Join-Path $PSScriptRoot "AppxManifest.xml"))
$manifest.Package.Identity.Version = "$version.0"
if ($StoreName) { $manifest.Package.Identity.Name = $StoreName }
if ($StorePublisher) { $manifest.Package.Identity.Publisher = $StorePublisher }
if ($StorePublisherDisplayName) { $manifest.Package.Properties.PublisherDisplayName = $StorePublisherDisplayName }
$manifest.Save((Join-Path $stage "AppxManifest.xml"))

$logos = @(
    @{ Name = "Square44x44Logo"; Size = 44 },
    @{ Name = "Square150x150Logo"; Size = 150 },
    @{ Name = "StoreLogo"; Size = 50 }
)
$files = @{}
foreach ($logo in $logos) {
    foreach ($scale in 100, 125, 150, 200, 400) {
        $files["$($logo.Name).scale-$scale.png"] = [int][math]::Floor(($logo.Size * $scale + 50) / 100)
    }
}
foreach ($target in 16, 24, 32, 48, 256) {
    $files["Square44x44Logo.targetsize-$target.png"] = $target
    $files["Square44x44Logo.targetsize-${target}_altform-unplated.png"] = $target
}
$sizes = ($files.Values | Sort-Object -Unique) -join ","
Push-Location $app
try {
    Invoke-Tool (Join-Path $app "node_modules\.bin\tauri.cmd") @("icon", "app-icon.svg", "--png", $sizes, "-o", $icons)
} finally {
    Pop-Location
}
foreach ($file in $files.GetEnumerator()) {
    Copy-Item (Join-Path $icons "$($file.Value)x$($file.Value).png") (Join-Path $stage "Assets\$($file.Key)")
}

$makepri = Find-SdkTool "makepri.exe"
$config = Join-Path $out "priconfig.xml"
Invoke-Tool $makepri @("createconfig", "/cf", $config, "/dq", "lang-en-US", "/pv", "10.0.0", "/o")
# One package for every language and scale: split resource packs are only for bundles.
$priConfig = New-Object System.Xml.XmlDocument
$priConfig.Load($config)
$packaging = $priConfig.SelectSingleNode("/resources/packaging")
if ($packaging) { [void]$packaging.ParentNode.RemoveChild($packaging) }
$priConfig.Save($config)
Invoke-Tool $makepri @("new", "/pr", $stage, "/cf", $config, "/mn", (Join-Path $stage "AppxManifest.xml"), "/of", (Join-Path $stage "resources.pri"), "/o")

$package = Join-Path $out "Bediz_${version}_x64.msix"
Invoke-Tool (Find-SdkTool "makeappx.exe") @("pack", "/d", $stage, "/p", $package, "/o")
Write-Output "The package is $package; its files are in $stage"
