# Unpacks the 32-bit MiTeC tools from the mitec.cz zips into files/, install.cmd copies them to Program Files\MiTeC
files: $(wildcard *.zip)
	rm -rf $@
	mkdir $@
	for zip in $^; do unzip -q -o $$zip -x '*64.exe' -d $@; done
