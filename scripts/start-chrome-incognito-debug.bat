@echo off
setlocal

set "CHROME_EXE=%ProgramFiles%\Google\Chrome\Application\chrome.exe"
if not exist "%CHROME_EXE%" set "CHROME_EXE=%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe"

if not exist "%CHROME_EXE%" (
  echo Chrome executable was not found.
  echo Install Google Chrome or update CHROME_EXE in this script.
  exit /b 1
)

set "PROFILE_DIR=%TEMP%\pstmacro-chrome-incognito-debug"

start "" "%CHROME_EXE%" ^
  --remote-debugging-port=9222 ^
  --user-data-dir="%PROFILE_DIR%" ^
  --incognito ^
  --disable-quic ^
  --no-first-run ^
  --no-default-browser-check ^
  https://www.naver.com

echo Chrome incognito debugging session started.
echo DevTools endpoint: http://127.0.0.1:9222/json/version
echo Profile dir: %PROFILE_DIR%
