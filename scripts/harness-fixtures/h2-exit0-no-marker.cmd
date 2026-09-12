@echo off
rem H2 fixture: exits 0 but never prints the required business marker.
rem For steps that declare a semantic assertion, exit 0 alone must not be PASS.
echo running H2 fixture
echo step completed without the required marker
exit /b 0
