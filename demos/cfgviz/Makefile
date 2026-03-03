ELLE ?= ../../target/release/elle
DOTS := identity.dot factorial.dot fizzbuzz.dot make-adder.dot eval-expr.dot
SVGS := $(DOTS:.dot=.svg)

.PHONY: all clean dots

all: $(SVGS)

dots: $(DOTS)

$(DOTS): cfgviz.lisp
	cd ../.. && ./target/release/elle demos/cfgviz/cfgviz.lisp

%.svg: %.dot
	dot -Tsvg $< -o $@

clean:
	rm -f $(DOTS) $(SVGS)
