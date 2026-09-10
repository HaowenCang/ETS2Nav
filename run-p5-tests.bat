@echo off
rem P5 Regression Suite (PLAN-P3plus.md A3): od-corpus gates + known-routes baseline diff + random OD check
rem A3c-M3 (2026-08-12 audit fix): [1/4] compile failure no longer false-PASS (match error[E);
rem [3/4][4/4] prefixed by cargo build --release (verify fresh binary, not stale release)
rem 2026-08-12: DATASET -> europe-v5 (P5 ferry-terminal fix generation); baseline -> v5
rem 2026-09-11: per-step verdicts decoupled from cumulative FAIL (a first failure used to label every
rem             later step FAIL even when its own command succeeded); FAIL now only feeds the exit code.
rem             Note [3/4]: with no baseline the generation step was already gated to run only when the
rem             file is absent, so its verdict no longer depends on unrelated earlier steps.
setlocal
set DATASET=E:\Projects\Pi\ETS2Nav\data\europe-v5
set BASELINE=E:\Projects\Pi\ETS2Nav\od-baseline-europe-v5.txt
set FAIL=0

echo === [1/4] cargo test (od-corpus + deps) ===
cd nav-core
cargo test -p od-corpus 2>&1 | findstr /C:"FAILED" /C:"error[E" >nul
if errorlevel 1 (echo CARGO TEST PASS) else (echo CARGO TEST FAIL & set FAIL=1)
cargo test -p nav-router 2>&1 | findstr /C:"FAILED" /C:"error[E" >nul
if errorlevel 1 (echo ROUTER TEST PASS) else (echo ROUTER TEST FAIL & set FAIL=1)

echo === [2/4] fmt/clippy (od-corpus) ===
cargo fmt -p od-corpus --check >nul 2>&1
if errorlevel 1 (echo FMT FAIL & set FAIL=1) else (echo FMT PASS)
cargo clippy -p od-corpus --all-targets 2>&1 | findstr /C:"warning" /C:"error" >nul
if errorlevel 1 (echo CLIPPY PASS) else (echo CLIPPY FAIL & set FAIL=1)

echo === [2.5/4] cargo build --release (od-corpus) ===
cargo build --release -p od-corpus 2>&1 | findstr /C:"error" >nul
if errorlevel 1 (echo BUILD PASS) else (echo BUILD FAIL & set FAIL=1)

echo === [3/4] known-routes baseline diff ===
if exist %BASELINE% (
  echo BASELINE EXISTS
) else (
  target\release\od-corpus.exe od-baseline %DATASET% --write %BASELINE% 2>&1 | findstr /C:"OD-BASELINE PASS" >nul
  if errorlevel 1 (echo BASELINE GEN FAIL & set FAIL=1) else (echo BASELINE GEN PASS)
)
target\release\od-corpus.exe od-regress %DATASET% %BASELINE% | findstr /C:"OD-REGRESS PASS" >nul
if errorlevel 1 (echo OD REGRESS FAIL & set FAIL=1) else (echo OD REGRESS PASS)

echo === [4/4] random OD check (500 pairs smoke) ===
target\release\od-corpus.exe od-check %DATASET% 500 | findstr /C:"OD-CHECK PASS" >nul
if errorlevel 1 (echo OD CHECK FAIL & set FAIL=1) else (echo OD CHECK PASS)

cd ..
if %FAIL%==1 (
  echo P5 Regression Suite: FAIL
  exit /b 1
)
echo P5 Regression Suite: ALL PASS
exit /b 0
