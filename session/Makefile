# nottsd: the D-Bus front of the notification center. libsystemd, or elogind's sd-bus (Void).
CC ?= cc
CFLAGS ?= -O2 -Wall -Wextra
LIB := $(shell pkg-config --libs libsystemd 2>/dev/null || pkg-config --libs libelogind 2>/dev/null)
INC := $(shell pkg-config --cflags libsystemd 2>/dev/null || pkg-config --cflags libelogind 2>/dev/null)
PREFIX ?= $(HOME)/.local

nottsd: nottsd.c
	$(CC) $(CFLAGS) $(INC) -o $@ nottsd.c $(LIB)

install: nottsd
	install -D nottsd $(PREFIX)/bin/nottsd

test: nottsd
	./test-nottsd

.PHONY: install test
