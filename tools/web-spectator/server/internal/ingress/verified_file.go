package ingress

import (
	"io"
	"net/http"
	"time"
)

const privateImmutableCache = "private, max-age=31536000, immutable"

// A verified, content-addressed file can be kept by the authenticated browser,
// but a conditional or range error must not inherit its immutable cache policy.
func serveVerifiedFile(w http.ResponseWriter, r *http.Request, name, mime, etag string, content io.ReadSeeker) {
	w.Header().Set("Content-Type", mime)
	w.Header().Set("Cache-Control", privateImmutableCache)
	w.Header().Set("ETag", etag)
	http.ServeContent(verifiedFileWriter{ResponseWriter: w}, r, name, time.Time{}, content)
}

type verifiedFileWriter struct{ http.ResponseWriter }

func (w verifiedFileWriter) WriteHeader(status int) {
	if status != http.StatusOK && status != http.StatusPartialContent && status != http.StatusNotModified {
		w.Header().Set("Cache-Control", "private, no-store")
	}
	w.ResponseWriter.WriteHeader(status)
}
