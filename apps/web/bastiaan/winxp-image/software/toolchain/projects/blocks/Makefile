.DELETE_ON_ERROR:

blocks.exe: main.c materials.h world.h $(wildcard assets/*.png) resource.o Makefile
	tcc -Wall -DUNICODE -D_UNICODE main.c resource.o -ld3d8 -lgdiplus -lole32 -o $@

resource.o: resource.rc app.manifest Makefile
	windres $< $@
