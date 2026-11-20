.DELETE_ON_ERROR:

hello-world.exe: main.c resource.o Makefile
	tcc -Wall -DUNICODE -D_UNICODE main.c resource.o -lgdi32 -o $@

resource.o: resource.rc manifest.xml Makefile
	windres $< $@
