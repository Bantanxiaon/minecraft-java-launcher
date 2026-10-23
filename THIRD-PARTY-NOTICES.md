# Third-party notices

Additional MIT copyright texts: [MicroCom](licenses/MicroCom-MIT.txt), [Tmds.DBus](licenses/Tmds-DBus-MIT.txt), [Avalonia.BuildServices](licenses/Avalonia-BuildServices-MIT.txt).
The desktop shell uses the dependencies below. Preserve these notices and the accompanying license files when redistributing. Native components can include additional third-party terms; the bundled notice texts are retained.
| Package | Version | Declared license | Repository |
|---|---|---|---|
| Avalonia | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.Angle.Windows.Natives | 2.1.27548.20260419 | LICENSE | https://github.com/AvaloniaUI/angle/ |
| Avalonia.BuildServices | 11.3.2 | MIT | https://github.com/AvaloniaUI/Avalonia.BuildServices |
| Avalonia.Desktop | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.FreeDesktop | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.FreeDesktop.AtSpi | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.HarfBuzz | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.Native | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.Remote.Protocol | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.Skia | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.Themes.Fluent | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.Win32 | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| Avalonia.X11 | 12.1.4 | MIT | https://github.com/AvaloniaUI/Avalonia/ |
| HarfBuzzSharp | 8.3.1.3 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| HarfBuzzSharp.NativeAssets.Linux | 8.3.1.3 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| HarfBuzzSharp.NativeAssets.macOS | 8.3.1.3 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| HarfBuzzSharp.NativeAssets.WebAssembly | 8.3.1.3 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| HarfBuzzSharp.NativeAssets.Win32 | 8.3.1.3 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| MicroCom.Runtime | 0.11.6 | MIT | https://github.com/kekekeks/MicroCom |
| SkiaSharp | 3.119.4 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| SkiaSharp.NativeAssets.Linux | 3.119.4 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| SkiaSharp.NativeAssets.macOS | 3.119.4 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| SkiaSharp.NativeAssets.WebAssembly | 3.119.4 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| SkiaSharp.NativeAssets.Win32 | 3.119.4 | MIT | https://go.microsoft.com/fwlink/?linkid=868515 |
| Tmds.DBus.Protocol | 0.94.1 | MIT | https://github.com/tmds/Tmds.DBus.git |

Avalonia license: [MIT](licenses/Avalonia-MIT.txt).
Additional texts:
- [licenses/Avalonia.Angle.Windows.Natives-LICENSE](licenses/Avalonia.Angle.Windows.Natives-LICENSE)
- [licenses/HarfBuzzSharp-LICENSE.txt](licenses/HarfBuzzSharp-LICENSE.txt)
- [licenses/HarfBuzzSharp.NativeAssets.Linux-THIRD-PARTY-NOTICES.txt](licenses/HarfBuzzSharp.NativeAssets.Linux-THIRD-PARTY-NOTICES.txt)

## Launcher engine dependencies

| Package | Version | License | Source |
|---|---|---|---|
| CmlLib.Core | 4.1.0 | MIT | https://github.com/CmlLib/CmlLib.Core/tree/ffa2e908a9aa545c13175ec7b700472220e6d280 |
| CmlLib.Core.Commons | 4.0.0 | MIT | https://github.com/CmlLib/CmlLib.Core.Commons/tree/aa96916cde628dcd74d4e82b5bfb20589398af1e |
| SharpZipLib | 1.4.2 | MIT | https://github.com/icsharpcode/SharpZipLib/tree/v1.4.2 |
| LZMA-SDK | 19.0.0 | MIT (package wrapper); original SDK by Igor Pavlov | https://github.com/monemihir/LZMA-SDK |

Preserve [CmlLib.Core MIT](licenses/CmlLib.Core-MIT.txt), [Commons MIT](licenses/CmlLib.Core.Commons-MIT.txt), [SharpZipLib MIT](licenses/SharpZipLib-MIT.txt), and [LZMA package MIT](licenses/LZMA-SDK-MIT.txt).
Package copyright declarations: CmlLib.Core © 2023 AlphaBs; Commons © 2024 AlphaBs; SharpZipLib © 2000–2022 SharpZipLib Contributors; LZMA-SDK © 2020 Mihir Mone. Original LZMA SDK source is credited to Igor Pavlov. Preserve these alongside upstream license copyright years.
System.Text.Json and System.Threading.Tasks.Dataflow are pruned framework dependencies when targeting .NET 10; no separate runtime package is redistributed by this project.

Limited behavioral research also consulted [Plain Craft Launcher by 龙腾猫跃](https://github.com/Meloong-Git/PCL) and [MeloongCore](https://github.com/Meloong-Git/MeloongCore). They are reference material, not dependencies. No implementation or assets from those repositories are included. SH Launcher Next is an independent project.



CmlLib.Core.Installer.Forge 2.0.0 — MIT, Copyright (c) 2023 CmlLib; https://github.com/CmlLib/CmlLib.Core.Installer.Forge ; full license: licenses/CmlLib.Core.Installer.Forge-LICENSE.txt. Its transitive HtmlAgilityPack 1.11.48 — MIT, ZZZ Projects Inc.; https://github.com/zzzprojects/html-agility-pack ; full license: licenses/HtmlAgilityPack-LICENSE.txt. Only Engine adapter references these packages.


SharpZipLib 1.4.2 is also used directly by Infrastructure for bounded CRC-checked archive inspection/copying. The MIT notice above applies. BMCLAPI is an optional artifact mirror service; no OpenBMCLAPI source is bundled.

UI download and folder-input icon paths derive from Lucide, commit 20aef31db93fe4d9f398dbb15b120d99c48eef95. ISC; Feather-derived icons additionally MIT. Original SVGs: assets/icons/lucide/. Preserve licenses/Lucide-LICENSE.txt. No Lucide runtime package is used.

## XMCL offline identity algorithm
C# adaptation of xmcl-runtime/user/utils/offlineUser.ts, Voxelum/x-minecraft-launcher (master, reviewed 2026-10-22). Copyright (c) 2023 ci010. MIT license retained in licenses/XMCL-LICENSE.txt. Standard OfflinePlayer MD5 UUID v3/variant algorithm; no other launcher source copied.

## authlib-injector
Official unmodified Java Agent obtained from yushijinhun/authlib-injector official distribution. AGPLv3-or-later with AUTHLIB-INJECTOR exception. Full text: licenses/authlib-injector-LICENSE.txt. No AGPL source linked or copied into launcher.

## Shared animated background
AnimationImage.Avalonia 3.1.1 — MIT; https://github.com/LazyCuteLion/AnimationImage ; full license licenses/AnimationImage-LICENSE.txt. Upstream HEAD reviewed at b30d443a8a5658ed8522c911b6ce97bcdd5042e2. NuGet source metadata names 538b4fa16e5d87e2ff6b848f929cd051e5fc7df5, which was unavailable through GitHub during review; exact package content is pinned in packages.lock.json. On-demand CPU WebP decoding is used; unused native transitive libraries remain subject to their notices.

Silk.NET Core, Direct3D12, DXGI, Maths, SDL 2.22.0 — MIT (licenses/Silk.NET-LICENSE.txt), https://github.com/dotnet/Silk.NET/tree/f9535d2900ac226ee6c52cf96a7110fc1115730c . Ultz.Native.SDL 2.30.8 — zlib (licenses/SDL-LICENSE.txt), https://github.com/libsdl-org/SDL/tree/release-2.30.8 .

SkiaSharp Skottie, Resources, SceneGraph 3.119.2 — MIT (licenses/SkiaSharp-LICENSE.txt), https://github.com/mono/SkiaSharp/tree/2888c737ad016d584c74525e2d35db5097ea8576 . Microsoft.Extensions.DependencyModel 8.0.0 — MIT (licenses/Microsoft-DotNet-Runtime-LICENSE.txt). Microsoft.DotNet.PlatformAbstractions 3.1.6 — MIT (licenses/Microsoft-DotNet-PlatformAbstractions-LICENSE.txt).

Polar Trails background derives from Alejandro De Roa, Pexels video 30039803, https://www.pexels.com/video/mesmerizing-night-sky-with-star-trails-30039803/ . Pexels License: https://www.pexels.com/license/ . Used as an integrated launcher background, not redistributed as a stock/wallpaper catalogue. Modifications: 16:9 crop, one-fifth playback speed, slight color adjustment, six-second cyclic crossfade, silent animated WebP and static poster. Pexels prohibits redistribution on other stock-photo or wallpaper platforms and use as a trademark; those permissions are not claimed.


## New Presentation Shell dependencies

The presentation shell uses Irihi.Ursa 2.2.0 and Irihi.Ursa.Themes.Semi 2.2.0 (MIT, copyright .NET Foundation and Contributors), Semi.Avalonia 12.0.1 (MIT, copyright IRIHI Technology), Irihi.Avalonia.Shared and .Contracts 0.5.0 (MIT, copyright iriHi Technology). Full notices are retained in licenses/*-LICENSE.txt.

Shell/ToastTheme.axaml is adapted from Ursa.Avalonia 2.2.0, commit 00fef728ce87cc8cbc9085f3162867daa1d57490, src/Ursa.Themes.Semi/Controls/Toast.axaml. The adaptation adds Reduced Motion conditions and immediate reduced-motion close; Ursa still owns toast lifetime and layout.

Shell/WindowDecorations.axaml adapts the Avalonia 12.1.4 Fluent window decoration template (MIT, AvaloniaUI contributors). The separate close-button derived-theme pattern was also reviewed in Fluid.Avalonia, commit 006eb180a4f7ed2dca5404b98e07b076d1d5f016 (MIT, Alpaq92); its full license is retained in licenses/Fluid.Avalonia-LICENSE.txt. No FluidWindow implementation or launcher business code was copied.

## Tomlyn 2.10.1
Forge/NeoForge TOML metadata parser. Copyright (c) 2019-2026 Alexandre Mutel. BSD-2-Clause; see licenses/Tomlyn.txt. https://github.com/xoofx/Tomlyn/tree/2.10.1

## XMCL architecture reference
Account texture capabilities, Minecraft face UV layout and mod metadata normalization were researched against Voxelum/x-minecraft-launcher commit 029f9ba466c181aebe5884439e4c65b481b175e3 (MIT). No TypeScript runtime included. See licenses/XMCL.txt.
