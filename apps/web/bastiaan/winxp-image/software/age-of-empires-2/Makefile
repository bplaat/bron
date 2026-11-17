# Installs Age of Empires II and The Conquerors into root/ like their setups do (full install, see ../../ebu.py), the
# game data comes from the CABs, winxp-image/disk.mjs copies root/ onto the C: drive
APP := Program Files/Microsoft Games/Age of Empires II

root: cd/AOE2/AOESETUP.EXE cd/AOE2CONQ/AOCSETUP.EXE ../../ebu.py Makefile
	rm -rf $@
	python3 ../../ebu.py cd/AOE2/AOESETUP.EXE cd/AOE2/SETUPENU.DLL cd/AOE2 $@ "$(APP)"
	cabextract -q -d "$@/$(APP)" cd/AOE2/GAME/EMPIRES2.CAB
	python3 ../../ebu.py cd/AOE2CONQ/AOCSETUP.EXE cd/AOE2CONQ/STPENUX.DLL cd/AOE2CONQ $@ "$(APP)"
	cabextract -q -d "$@/$(APP)" cd/AOE2CONQ/MSGAME.CAB
