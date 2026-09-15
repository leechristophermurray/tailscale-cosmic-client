# COPR's "make srpm" build method runs this in a fresh clone of the repository:
#   make -f .copr/Makefile srpm outdir=<dir> spec=<path>
#
# It packs the checked-out commit as the tarball the spec expects, so a COPR
# build of a tag and a local build of the same commit use identical sources.

spec ?= packaging/rpm/cosmic-tailscale.spec
outdir ?= $(CURDIR)/dist

srpm:
	command -v git >/dev/null && command -v rpmbuild >/dev/null || dnf -y install git rpm-build
	git config --global --add safe.directory '*'
	version=$$(sed -n 's/^Version:[[:space:]]*//p' $(spec)); \
	mkdir -p $(outdir) && \
	git archive --format=tar.gz --prefix=tailscale-cosmic-client-$$version/ \
		--output=$(outdir)/tailscale-cosmic-client-$$version.tar.gz HEAD && \
	rpmbuild -bs --define "_sourcedir $(outdir)" --define "_srcrpmdir $(outdir)" $(spec)

.PHONY: srpm
