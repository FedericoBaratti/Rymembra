#!/usr/bin/env bash
# =============================================================================
# Aether - build APK "singolo click"
# -----------------------------------------------------------------------------
# Builda l'APK debug dal codice Windows attuale (repo autoritativo) nell'albero
# WSL ~/aether, poi lo copia in Desktop\Apk con il nome:
#     Aether v0.9.<giorno>.<mese>.26.apk
# calcolato dalla data odierna. Se il nome esiste gia' accoda .2, .3, ...
# Prima della build allinea la "version" di package.json alla stessa data, cosi'
# la versione mostrata in-app combacia col nome del file.
#
# Va eseguito DENTRO WSL (Ubuntu). Il launcher Windows "Aether - Build APK.bat"
# lo invoca via:  wsl -e bash -lc "bash '<questo file su /mnt/c/...>'"
#
# Procedura di build: cfr. memoria android-apk-build-procedure.
# =============================================================================
set -euo pipefail

# --- 1. Ambiente -------------------------------------------------------------
# nvm node v20.20.2: senza questo, sotto `bash -lc` npm risolve al npm di Windows
# (via interop) e fallisce sul cwd UNC.
export PATH="$HOME/.nvm/versions/node/v20.20.2/bin:$PATH"

# Repo Windows autoritativo (montato su /mnt/c). I sync script lo leggono da qui.
export AETHER_WIN_REPO="/mnt/c/Users/Federico Baratti/Desktop/Nuova cartella/Aeter - porting android"

WSL_TREE="$HOME/aether"                                   # albero di build WSL
OUT_DIR="/mnt/c/Users/Federico Baratti/Desktop/Apk"       # destinazione APK

# --- 2. Nome / versione dalla data odierna -----------------------------------
DAY="$(date +%-d)"      # giorno senza zero iniziale (es. 11)
MONTH="$(date +%-m)"    # mese senza zero iniziale (es. 7)
VERSION="0.9.$DAY.$MONTH.26"
BASENAME="Aether v$VERSION"
echo "==> Versione/nome del giorno: $BASENAME"

# --- 3. Allinea la version di package.json (repo Windows) --------------------
# sed mirato alla PRIMA occorrenza per preservare la formattazione del file.
echo "==> Aggiorno package.json -> \"version\": \"$VERSION\""
sed -i -E "0,/\"version\":/ s/(\"version\": \")[^\"]*\"/\1$VERSION\"/" \
  "$AETHER_WIN_REPO/package.json"

# --- 4. Sync sorgente Windows -> WSL (Windows autoritativo) -------------------
# rsync SENZA --delete: eventuali file orfani solo-WSL sopravvivono (innocui).
# Escludiamo node_modules, *.tsbuildinfo e android/ (la native build nodejs-mobile
# nel tree WSL e' hand-maintained; i .kt/res arrivano via sync:android-native).
echo "==> Sync sorgente Windows -> WSL ($WSL_TREE)"
rsync -a --exclude='node_modules' --exclude='*.tsbuildinfo' --exclude='android' \
  "$AETHER_WIN_REPO/src" \
  "$AETHER_WIN_REPO/shared" \
  "$AETHER_WIN_REPO/node-backend" \
  "$AETHER_WIN_REPO/electron" \
  "$AETHER_WIN_REPO/scripts" \
  "$WSL_TREE/"

# File di config root necessari alla build mobile/node.
for f in package.json \
         vite.config.mobile.ts vite.config.node-backend.ts \
         capacitor.config.ts \
         tsconfig.json tsconfig.web.json tsconfig.node.json tsconfig.node-backend.json \
         index.mobile.html; do
  rsync -a "$AETHER_WIN_REPO/$f" "$WSL_TREE/"
done

# --- 4b. Dipendenze ----------------------------------------------------------
# Il sync porta package.json ma NON node_modules: una dipendenza aggiunta su
# Windows (es. @fontsource-*) senza questo step rompe build:mobile con
# "Rollup failed to resolve import". npm install e' quasi no-op se allineato.
cd "$WSL_TREE"
echo "==> npm install (allinea node_modules a package.json)"
npm install --no-audit --no-fund

# --- 5. Build ----------------------------------------------------------------
echo "==> npm run deploy:android (build renderer + backend + sync + preflight)"
npm run deploy:android

echo "==> Dedup jniLibs libnode.so / libnode.so.gz (idempotente)"
node scripts/sync-cordova-nodejs-native.mjs

echo "==> gradlew clean assembleDebug (puo' richiedere qualche minuto)"
cd android
./gradlew clean assembleDebug

# --- 6. Copia + nomenclatura + gestione collisioni ---------------------------
APK_SRC="$WSL_TREE/android/app/build/outputs/apk/debug/app-debug.apk"
if [ ! -f "$APK_SRC" ]; then
  echo "ERRORE: APK non trovato in $APK_SRC" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
DEST="$OUT_DIR/$BASENAME.apk"
n=2
while [ -e "$DEST" ]; do
  DEST="$OUT_DIR/$BASENAME.$n.apk"
  n=$((n + 1))
done

cp "$APK_SRC" "$DEST"
echo ""
echo "==> APK creato: $DEST"
