# Installs Age of Empires II and The Conquerors into root/ like their setups do (full install, see ../../ebu.mjs), the
# game data comes from the CABs, winxp-image/disk.mjs copies root/ onto the C: drive
APP := Program Files/Microsoft Games/Age of Empires II

root: cd/AOE2/AOESETUP.EXE cd/AOE2CONQ/AOCSETUP.EXE ../../ebu.mjs Makefile
	rm -rf $@
	../../ebu.mjs cd/AOE2/AOESETUP.EXE cd/AOE2/SETUPENU.DLL cd/AOE2 $@ "$(APP)"
	bsdtar -xf cd/AOE2/GAME/EMPIRES2.CAB -C "$@/$(APP)"
	../../ebu.mjs cd/AOE2CONQ/AOCSETUP.EXE cd/AOE2CONQ/STPENUX.DLL cd/AOE2CONQ $@ "$(APP)"
	bsdtar -xf cd/AOE2CONQ/MSGAME.CAB -C "$@/$(APP)"
