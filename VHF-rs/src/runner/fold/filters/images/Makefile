SRCFILES = $(wildcard *.tex)
IMAGES = $(patsubst %.tex, %.png, $(SRCFILES))

.PHONY: clean all

all: $(IMAGES)

clean:
	$(RM) $(IMAGES)
	rm -f *.aux *.log *.bbl *.blg *.out *.toc *.fdb_latexmk *.fls *.synctex.gz *.pdf

clean_aux:
	rm -f *.aux *.log *.bbl *.blg *.out *.toc *.fdb_latexmk *.fls *.synctex.gz

%.png: %.tex
# shell-escape needed to emit as png
# -halt-on-error for easier Makefile
	pdflatex -interaction=nonstopmode -halt-on-error -shell-escape $< < /dev/null
