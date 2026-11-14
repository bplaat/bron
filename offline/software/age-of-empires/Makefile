# Installs Age of Empires and The Rise of Rome into root/ like their setups do (full install, see ../../ebu.py),
# offline/disk.mjs copies root/ onto the C: drive
APP := Program Files/Microsoft Games/Age of Empires

root: cd/AOE1/AOESETUP.EXE cd/AOE1ROR/AOESETUP.EXE ../../ebu.py Makefile
	rm -rf $@
	python3 ../../ebu.py cd/AOE1/AOESETUP.EXE cd/AOE1/SETUPENU.DLL cd/AOE1 $@ "$(APP)"
	python3 ../../ebu.py cd/AOE1ROR/AOESETUP.EXE cd/AOE1ROR/SETUPEXP.DLL cd/AOE1ROR $@ "$(APP)"
