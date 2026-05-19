DOCS := protocol-spec protocol-overview
PDFS := $(addsuffix .pdf,$(DOCS))

.PHONY: all clean

all: $(PDFS)

%.pdf: %.tex references.bib
	pdflatex -interaction=nonstopmode $<
	bibtex $(basename $<)
	pdflatex -interaction=nonstopmode $<
	pdflatex -interaction=nonstopmode $<

clean:
	rm -f $(foreach d,$(DOCS),$(d).aux $(d).bbl $(d).blg $(d).log $(d).out $(d).toc)
