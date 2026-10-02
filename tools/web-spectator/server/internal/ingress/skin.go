package ingress

import (
	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
	"net/http"
	"time"
)

func (h *Handler) skin(w http.ResponseWriter, r *http.Request, id, hash string, live *spectator.Live) {
	var playerID string
	valid, _ := live.WithCurrent(time.Now(), func(frame spectator.Frame, _ *spectator.Arena) error {
		for _, p := range frame.Players {
			if p.Bot && p.SkinID == hash {
				playerID = p.ID
				break
			}
		}
		return nil
	})
	if !valid || playerID == "" {
		failure(w, http.StatusNotFound, "This duel skin is unavailable.")
		return
	}
	data := h.store.Skin(id, playerID, hash, time.Now())
	if len(data) == 0 {
		failure(w, http.StatusNotFound, "This duel skin is unavailable.")
		return
	}
	_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(writeTimeout))
	served := false
	valid, _ = live.WithCurrent(time.Now(), func(frame spectator.Frame, _ *spectator.Arena) error {
		for _, p := range frame.Players {
			if p.ID == playerID && p.Bot && p.SkinID == hash {
				served = true
				w.Header().Set("Content-Type", "image/png")
				if r.Method == http.MethodHead {
					w.WriteHeader(http.StatusOK)
					return nil
				}
				_, err := w.Write(data)
				return err
			}
		}
		return nil
	})
	if !valid || !served {
		failure(w, http.StatusNotFound, "This duel skin is unavailable.")
	}
}
