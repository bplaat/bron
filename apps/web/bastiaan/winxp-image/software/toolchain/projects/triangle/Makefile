.DELETE_ON_ERROR:

triangle.exe: main.c resource.o Makefile
	tcc -Wall -DUNICODE -D_UNICODE main.c resource.o -ld3d8 -o $@

resource.o: resource.rc app.manifest Makefile
	windres $< $@
