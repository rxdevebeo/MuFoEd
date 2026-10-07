@echo off
setlocal EnableExtensions
rem hostq.bat - launcher for hostq.ps1 (StrictLib host job executor).
rem
rem Usage: hostq.bat [repo] [-AllowPush] [-SelfTest]
rem
rem The repo is optional. Precedence: first argument, then %HOSTQ_REPO%,
rem then the repository this file lives in: two levels above tools\hostq
rem (D:\projects\StrictLib when run from the main copy).
rem
rem Run with no arguments to start the executor in the foreground.

set "PS1=%~dp0hostq.ps1"
if not exist "%PS1%" (
  echo hostq: missing "%PS1%" 1>&2
  exit /b 2
)

rem The host ran out of memory compiling test binaries in parallel; cap the
rem build jobs unless the owner set a value of their own.
if not defined CARGO_BUILD_JOBS set "CARGO_BUILD_JOBS=4"

set "REPO=%~1"
set "REST="

if "%REPO%"=="" goto defaults

rem A leading '-' means the first token is a flag, not the repo.
if "%REPO:~0,1%"=="-" (
  set "REPO="
  set "REST= %*"
  goto defaults
)

rem The first token is the repo; collect the remaining flags.
shift
:collect
if "%~1"=="" goto defaults
set "REST=%REST% %1"
shift
goto collect

:defaults
if "%REPO%"=="" if defined HOSTQ_REPO set "REPO=%HOSTQ_REPO%"
if "%REPO%"=="" for %%I in ("%~dp0..\..") do set "REPO=%%~fI"

echo hostq: repo=%REPO% CARGO_BUILD_JOBS=%CARGO_BUILD_JOBS%
powershell -NoProfile -ExecutionPolicy Bypass -File "%PS1%" -Repo "%REPO%"%REST%
exit /b %ERRORLEVEL%
