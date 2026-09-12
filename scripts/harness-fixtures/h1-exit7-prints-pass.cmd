@echo off
rem H1 fixture: prints PASS but exits 7. The harness must judge this FAIL.
rem The harness's oracle is the process exit code; a success-looking token in
rem stdout must never override it.
echo running H1 fixture
echo PASS
echo ALL TESTS PASSED
exit /b 7
