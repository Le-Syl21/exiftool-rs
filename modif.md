# CI Corrections — exiftool-rs

## 1. 🔴 Ajouter `stapler` après notarisation (CRITIQUE)

**Fichier :** `.github/workflows/ci.yml`

**Problème :** Le ticket de notarisation Apple n'est pas collé dans les binaires avant packaging.
Les utilisateurs qui extraient le `.tar.gz` hors ligne tombent sur un avertissement Gatekeeper.

**Insérer ce step entre `Notarize binaries` et `Package CLI (Unix)` :**

```yaml
- name: Staple notarization tickets (macOS)
  if: runner.os == 'macOS' && startsWith(github.ref, 'refs/tags/v')
  run: |
    xcrun stapler staple target/release/exiftool-rs
    xcrun stapler staple target/release/exiftool-rs-gui
```

---

## 2. 🟡 Paralléliser les soumissions `notarytool`

**Fichier :** `.github/workflows/ci.yml`, step `Notarize binaries (macOS, tags only)`

**Problème :** Les deux soumissions sont séquentielles (~4-5 min chacune), soit ~8-10 min au total.

**Remplacer le bloc `run` du step notarize par :**

```yaml
run: |
  ditto -c -k --keepParent \
    target/release/exiftool-rs \
    $RUNNER_TEMP/exiftool-rs-cli.zip
  ditto -c -k --keepParent \
    target/release/exiftool-rs-gui \
    $RUNNER_TEMP/exiftool-rs-gui.zip

  xcrun notarytool submit $RUNNER_TEMP/exiftool-rs-cli.zip \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_ID_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait &
  xcrun notarytool submit $RUNNER_TEMP/exiftool-rs-gui.zip \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_ID_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait &
  wait
```

**Gain estimé : ~4-5 min par tag push.**

---

## 3. 🟢 Vérifier les entitlements du GUI (selon fonctionnalités)

**Fichier :** `.github/workflows/ci.yml`, step `Sign binaries (macOS)`

**Problème :** Le Hardened Runtime sans entitlements peut bloquer certaines opérations à l'exécution
si le GUI accède au réseau, ouvre des dialogs natifs de fichiers, utilise la caméra/micro, etc.

**Si nécessaire, créer `codesign/gui.entitlements` et l'ajouter au codesign du GUI :**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <!-- Décommenter selon les besoins réels du GUI -->
  <!-- <key>com.apple.security.network.client</key><true/> -->
  <!-- <key>com.apple.security.files.user-selected.read-write</key><true/> -->
</dict>
</plist>
```

```yaml
- name: Sign binaries (macOS)
  run: |
    SIGN_ID="Developer ID Application: Sylvain Gargasson ($APPLE_TEAM_ID)"
    codesign --force --options runtime \
      --sign "$SIGN_ID" \
      --timestamp \
      target/release/exiftool-rs
    codesign --force --options runtime \
      --entitlements codesign/gui.entitlements \
      --sign "$SIGN_ID" \
      --timestamp \
      target/release/exiftool-rs-gui
```

**À ne faire que si des blocages runtime sont constatés sur macOS.**
