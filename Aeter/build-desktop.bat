@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title Aether - Build desktop

rem ==========================================================================
rem  Aether - Build desktop (Windows)
rem  Esegue in ordine: prerequisiti -> binari -> dipendenze -> typecheck ->
rem  pulizia -> build + packaging (NSIS + portable) in release\.
rem  Si ferma con messaggio chiaro al primo errore.
rem ==========================================================================

rem Root del progetto = cartella di questo script.
cd /d "%~dp0"

echo.
echo ==========================================================
echo   AETHER - BUILD DESKTOP
echo   %CD%
echo ==========================================================
echo.

rem ----------------------------------------------------------------------
rem  [1/6] Prerequisiti: node e npm nel PATH
rem ----------------------------------------------------------------------
set "STAGE=PREREQUISITI"
echo [1/6] Controllo prerequisiti (node / npm)...

where node >nul 2>nul
if errorlevel 1 (
  echo   ERRORE: 'node' non trovato nel PATH. Installa Node.js e riprova.
  goto :error
)
where npm >nul 2>nul
if errorlevel 1 (
  echo   ERRORE: 'npm' non trovato nel PATH. Installa Node.js/npm e riprova.
  goto :error
)

for /f "delims=" %%v in ('node -v') do echo   node %%v
for /f "delims=" %%v in ('npm -v')  do echo   npm  %%v
echo.

rem ----------------------------------------------------------------------
rem  [2/6] Binari esterni (warning, non fatale)
rem ----------------------------------------------------------------------
set "STAGE=BINARI ESTERNI"
echo [2/6] Controllo binari esterni in resources\bin ...

set "MISSING_BIN="
for %%B in (ffmpeg.exe yt-dlp.exe spotdl.exe fpcalc.exe) do (
  if not exist "resources\bin\%%B" set "MISSING_BIN=!MISSING_BIN! %%B"
)
if defined MISSING_BIN (
  echo   ATTENZIONE: binari mancanti:!MISSING_BIN!
  echo   La build procede lo stesso, ma download/conversione non funzioneranno
  echo   finche' non copi i file in resources\bin\ ^(vedi README^).
) else (
  echo   OK: ffmpeg, yt-dlp, spotdl, fpcalc presenti.
)
echo.

rem ----------------------------------------------------------------------
rem  [3/6] Dipendenze (solo se node_modules manca)
rem ----------------------------------------------------------------------
set "STAGE=DIPENDENZE"
if not exist "node_modules" (
  echo [3/6] node_modules assente: installo le dipendenze...
  call npm install
  if errorlevel 1 (
    echo   npm install fallito. Provo il fallback per moduli nativi ^(better-sqlite3^)...
    call npm install --ignore-scripts
    if errorlevel 1 goto :error
    call npx electron-builder install-app-deps
    if errorlevel 1 goto :error
  )
) else (
  echo [3/6] node_modules presente: salto l'installazione.
  echo       ^(per forzare una reinstallazione pulita cancella la cartella node_modules^)
)
echo.

rem ----------------------------------------------------------------------
rem  [4/6] Typecheck (gate)
rem ----------------------------------------------------------------------
set "STAGE=TYPECHECK"
echo [4/6] Typecheck TypeScript ^(node + web^)...
call npm run typecheck
if errorlevel 1 goto :error
echo   OK: typecheck superato.
echo.

rem ----------------------------------------------------------------------
rem  [5/6] Pulizia output vite
rem ----------------------------------------------------------------------
set "STAGE=PULIZIA"
echo [5/6] Pulizia cartella out\ ...
if exist "out" rmdir /s /q "out"
echo   OK.
echo.

rem ----------------------------------------------------------------------
rem  [6/6] Build + packaging Windows
rem ----------------------------------------------------------------------
set "STAGE=BUILD + PACKAGING"
echo [6/6] Build + packaging Windows ^(NSIS + portable^)...
call npm run dist:win
if errorlevel 1 goto :error
echo.

rem ----------------------------------------------------------------------
rem  Esito
rem ----------------------------------------------------------------------
echo ==========================================================
echo   BUILD COMPLETATA
echo ==========================================================
echo   Artefatti in: %CD%\release
echo.
if exist "release" (
  for /f "delims=" %%F in ('dir /b "release\*.exe" 2^>nul') do echo     - %%F
)
echo.
if exist "release" start "" explorer "release"
echo Fatto.
echo.
pause
endlocal
exit /b 0

rem ----------------------------------------------------------------------
:error
echo.
echo ==========================================================
echo   BUILD FALLITA  ^(stadio: %STAGE%^)
echo ==========================================================
echo   Controlla i messaggi qui sopra per la causa.
echo.
pause
endlocal
exit /b 1
