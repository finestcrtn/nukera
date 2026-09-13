@echo off
setlocal
cd /d "%~dp0"
set "PY=%~dp0tools\python\python.exe"
if not exist "%PY%" set "PY=python"
if "%~1"=="" (
    "%PY%" "%~dp0cli\nukera.py" menu
) else (
    "%PY%" "%~dp0cli\nukera.py" %*
)
exit /b %errorlevel%