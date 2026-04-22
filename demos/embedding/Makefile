.PHONY: chost clean

# Find the cdylib in the workspace target directory
TARGET_DIR ?= ../../target/debug
ABS_TARGET := $(abspath $(TARGET_DIR))

chost: host.c include/elle.h
	$(CC) -o chost host.c -Iinclude -L$(ABS_TARGET) -lelle_embed -Wl,-rpath,$(ABS_TARGET)

clean:
	rm -f chost
