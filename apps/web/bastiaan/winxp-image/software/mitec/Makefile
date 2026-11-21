# Unpacks the 32-bit MiTeC tools from the mitec.cz zips into files/, install.cmd copies them to Program Files\MiTeC
files: $(wildcard *.zip)
	rm -rf $@
	mkdir $@
	for zip in $^; do bsdtar -xf $$zip --exclude '*64.exe' -C $@; done
