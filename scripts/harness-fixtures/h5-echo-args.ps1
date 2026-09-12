# H5 fixture (ASCII only on purpose): echoes its working directory and its first
# two arguments verbatim, so the harness can prove that paths and arguments
# containing spaces arrive unmangled.
#
# Why a .ps1 fixture instead of the .cmd one: cmd.exe applies its own extra
# de-quoting rule to `cmd /c "<quoted path>" ...` which strips the outer quotes
# and splits the path at the first space. That is a cmd-specific hazard, not a
# property of ProcessStartInfo argument passing, and the regression suites never
# route a real step through cmd.exe.
Write-Output "CWD=[$((Get-Location).Path)]"
Write-Output "ARG1=[$($args[0])]"
Write-Output "ARG2=[$($args[1])]"
Write-Output "ARGC=$($args.Count)"
exit 0
