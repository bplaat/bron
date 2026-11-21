# 7-Zip's installer does not run on Windows XP, its files do: extract them into root/, winxp-image/disk.mjs copies root/
# onto the C: drive and install.cmd registers the shell extension
root: 7z2605.exe Makefile
	rm -rf $@
	mkdir -p "$@/Program Files/7-Zip"
	7zz x -y -o"$@/Program Files/7-Zip" $< -x!Uninstall.exe >/dev/null
