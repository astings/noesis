# Noesis macOS App Icon

## Xcode asset catalog

Copy `Assets.xcassets/AppIcon.appiconset` into the app's asset catalog, replacing the existing macOS AppIcon set when appropriate.

## Apple Icon Composer

Import `Noesis-Background-1024.png` as the full-canvas background and `Noesis-Ribbon-1024.png` as the foreground. Keep both layers centered, and do not add another mask to the source layers. These PNGs are import-ready layers, not a native `.icon` document.

Save the composition as a `.icon` file, add it to the Xcode target, and select its matching app-icon name in the target settings instead of the asset-catalog `AppIcon`.

## Manual or legacy bundle packaging

Add `Noesis.icns` to the app bundle and reference it with `CFBundleIconFile` when asset catalogs are not used.

## Source files

`Noesis-AppIcon-1024.png` is the conventional precomposed master; the separate background and ribbon layers support recomposition. `Noesis-AppIcon-Preview.png` is only a review sheet, not an app asset.
