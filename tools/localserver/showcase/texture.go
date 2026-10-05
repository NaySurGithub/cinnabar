package showcase

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"math"
)

// textureSize matches the UV layout of the vanilla warden geometry the boss reuses.
const textureSize = 128

// bossTextures returns the boss's body texture and its glowing vein layer as PNGs. Both are
// generated from value noise: ash-dark hide with gold fissures, the same fissures lit in the glow.
func bossTextures() (body, glow []byte, err error) {
	bodyImg := image.NewNRGBA(image.Rect(0, 0, textureSize, textureSize))
	glowImg := image.NewNRGBA(image.Rect(0, 0, textureSize, textureSize))
	for y := range textureSize {
		for x := range textureSize {
			fx, fy := float64(x)/textureSize, float64(y)/textureSize
			hide := fbm(fx*6, fy*6, 1)
			vein := math.Abs(fbm(fx*4+7.3, fy*4+1.9, 2) - 0.5)
			shade := 0.3 + 0.6*hide + 0.1*float64(hash(x, y, 3)%16)/15
			c := color.NRGBA{R: uint8(28 * shade), G: uint8(24 * shade), B: uint8(30 * shade), A: 255}
			var g color.NRGBA
			switch {
			case vein < 0.01:
				c = color.NRGBA{R: 232, G: 176, B: 58, A: 255}
				g = color.NRGBA{R: 255, G: 200, B: 80, A: 255}
			case vein < 0.022:
				c = color.NRGBA{R: 120, G: 84, B: 30, A: 255}
				g = color.NRGBA{R: 255, G: 170, B: 50, A: 110}
			case hash(x, y, 7)%97 == 0 && hide > 0.55:
				c = color.NRGBA{R: 200, G: 150, B: 60, A: 255} // gold flecks
			}
			bodyImg.SetNRGBA(x, y, c)
			glowImg.SetNRGBA(x, y, g)
		}
	}
	if body, err = encodePNG(bodyImg); err != nil {
		return nil, nil, err
	}
	glow, err = encodePNG(glowImg)
	return body, glow, err
}

// solidTexture is the white square the HUD tints into bars and slots.
func solidTexture() ([]byte, error) {
	img := image.NewNRGBA(image.Rect(0, 0, 4, 4))
	for i := range img.Pix {
		img.Pix[i] = 255
	}
	return encodePNG(img)
}

func encodePNG(img image.Image) ([]byte, error) {
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// fbm is four octaves of value noise in 0..1.
func fbm(x, y float64, seed uint32) float64 {
	sum, amp, norm := 0.0, 0.5, 0.0
	for octave := range 4 {
		sum += amp * valueNoise(x, y, seed+uint32(octave)*101)
		norm += amp
		x, y, amp = x*2, y*2, amp*0.5
	}
	return sum / norm
}

// valueNoise interpolates hashed lattice values smoothly.
func valueNoise(x, y float64, seed uint32) float64 {
	x0, y0 := math.Floor(x), math.Floor(y)
	tx, ty := smooth(x-x0), smooth(y-y0)
	ix, iy := int(x0), int(y0)
	at := func(dx, dy int) float64 {
		return float64(hash(ix+dx, iy+dy, int(seed))&0xffff) / 0xffff
	}
	top := at(0, 0)*(1-tx) + at(1, 0)*tx
	bottom := at(0, 1)*(1-tx) + at(1, 1)*tx
	return top*(1-ty) + bottom*ty
}

func smooth(t float64) float64 { return t * t * (3 - 2*t) }
