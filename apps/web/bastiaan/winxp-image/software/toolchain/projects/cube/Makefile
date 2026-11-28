.DELETE_ON_ERROR:

cube.exe: main.c crate.jpg resource.o Makefile
	tcc -Wall -DUNICODE -D_UNICODE main.c resource.o -ld3d8 -lgdiplus -lole32 -o $@

resource.o: resource.rc app.manifest Makefile
	windres $< $@
