# Installs Civilization III Complete into root/ on the host with innoextract, like its GOG setup does (the app/ part,
# without the DirectX 9 redistributable Civilization III does not need, the setup moves __support/save into place),
# offline/disk.mjs copies root/ onto the C: drive
APP := Program Files/Civilization III

root: setup_civilization3_complete_2.0.0.7.exe Makefile
	rm -rf $@ extract
	innoextract -q -s -I app -d extract $<
	mkdir -p "$@/Program Files"
	mv extract/app "$@/$(APP)"
	cp -R "$@/$(APP)/__support/save/." "$@/$(APP)/"
	rm -rf "$@/$(APP)/__support/save"
	rm -rf extract
