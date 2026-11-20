.DELETE_ON_ERROR:

clock.exe: main.c resource.o Makefile
	tcc -Wall -DUNICODE -D_UNICODE main.c resource.o -lgdi32 -lgdiplus -o $@

resource.o: resource.rc manifest.xml Makefile
	windres $< $@
