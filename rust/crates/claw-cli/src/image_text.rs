//! Une image en fichiers lisibles par un modèle qui ne lit que du texte.
//!
//! Le lecteur d'images (`image_reader`) fait décrire une image par un autre modèle : utile,
//! mais c'est son avis, et rien quand aucun lecteur ne répond. Ici, l'image est décodée
//! (PNG, JPEG de base, sans dépendance) et rendue telle qu'elle est :
//! - un texte : dimensions, palette des couleurs dominantes (code hexadécimal, nom, part),
//!   grille de couleurs (une lettre par case), zones de couleur (cadre en % et en pixels),
//!   et un dessin en caractères de la luminosité, qui montre les formes et les lignes de texte ;
//! - un SVG simplifié des zones de couleur, que le modèle peut lire et redessiner.
//!
//! Rien n'y est deviné : chaque valeur vient des pixels.

use std::fmt::Write as _;

// ============================================================================ pixels

/// Image décodée, en RVB (la transparence est posée sur du blanc).
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<[u8; 3]>,
    /// Une partie de l'image était transparente.
    pub transparent: bool,
}

/// Format reconnu d'après les premiers octets.
pub fn format_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else {
        "unknown"
    }
}

/// Décode une image PNG ou JPEG de base.
pub fn decode(bytes: &[u8]) -> Result<Pixels, String> {
    match format_of(bytes) {
        "png" => png::decode(bytes),
        "jpeg" => jpeg::decode(bytes),
        other => Err(format!(
            "{other} images cannot be decoded here (only PNG and baseline JPEG)"
        )),
    }
}

// ============================================================================ inflate

mod inflate {
    //! Décompression DEFLATE (RFC 1951), à la manière de `puff` de zlib.

    struct Bits<'a> {
        data: &'a [u8],
        pos: usize,
        buffer: u32,
        count: u32,
    }

    impl Bits<'_> {
        fn bits(&mut self, need: u32) -> Result<u32, String> {
            let mut value = self.buffer;
            while self.count < need {
                let byte = *self
                    .data
                    .get(self.pos)
                    .ok_or("compressed data ends too early")?;
                self.pos += 1;
                value |= u32::from(byte) << self.count;
                self.count += 8;
            }
            self.buffer = value >> need;
            self.count -= need;
            Ok(value & ((1u32 << need) - 1))
        }
    }

    struct Huffman {
        counts: [u16; 16],
        symbols: Vec<u16>,
    }

    impl Huffman {
        fn new(lengths: &[u8]) -> Self {
            let mut counts = [0u16; 16];
            for &length in lengths {
                counts[usize::from(length)] += 1;
            }
            counts[0] = 0;
            let mut offsets = [0u16; 16];
            for length in 1..15 {
                offsets[length + 1] = offsets[length] + counts[length];
            }
            let mut symbols = vec![0u16; lengths.len()];
            for (symbol, &length) in lengths.iter().enumerate() {
                if length != 0 {
                    let slot = &mut offsets[usize::from(length)];
                    symbols[usize::from(*slot)] = symbol as u16;
                    *slot += 1;
                }
            }
            Self { counts, symbols }
        }

        fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
            let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
            for length in 1..16 {
                code |= bits.bits(1)? as i32;
                let count = i32::from(self.counts[length]);
                if code - first < count {
                    return Ok(self.symbols[(index + code - first) as usize]);
                }
                index += count;
                first += count;
                first <<= 1;
                code <<= 1;
            }
            Err("invalid Huffman code in compressed data".to_string())
        }
    }

    const LENGTH_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    fn codes(
        bits: &mut Bits,
        out: &mut Vec<u8>,
        lengths: &Huffman,
        distances: &Huffman,
    ) -> Result<(), String> {
        loop {
            let symbol = lengths.decode(bits)?;
            match symbol {
                0..=255 => out.push(symbol as u8),
                256 => return Ok(()),
                _ => {
                    let index = usize::from(symbol - 257);
                    if index >= 29 {
                        return Err("invalid length in compressed data".to_string());
                    }
                    let length = usize::from(LENGTH_BASE[index])
                        + bits.bits(u32::from(LENGTH_EXTRA[index]))? as usize;
                    let code = usize::from(distances.decode(bits)?);
                    if code >= 30 {
                        return Err("invalid distance in compressed data".to_string());
                    }
                    let distance = usize::from(DIST_BASE[code])
                        + bits.bits(u32::from(DIST_EXTRA[code]))? as usize;
                    if distance > out.len() {
                        return Err("distance too far back in compressed data".to_string());
                    }
                    let start = out.len() - distance;
                    for offset in 0..length {
                        out.push(out[start + offset]);
                    }
                }
            }
        }
    }

    fn dynamic(bits: &mut Bits) -> Result<(Huffman, Huffman), String> {
        const ORDER: [usize; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];
        let literal_count = bits.bits(5)? as usize + 257;
        let distance_count = bits.bits(5)? as usize + 1;
        let code_count = bits.bits(4)? as usize + 4;
        let mut code_lengths = [0u8; 19];
        for &slot in ORDER.iter().take(code_count) {
            code_lengths[slot] = bits.bits(3)? as u8;
        }
        let code_huffman = Huffman::new(&code_lengths);
        let mut lengths = vec![0u8; literal_count + distance_count];
        let mut index = 0;
        while index < lengths.len() {
            let symbol = code_huffman.decode(bits)?;
            if symbol < 16 {
                lengths[index] = symbol as u8;
                index += 1;
                continue;
            }
            let (value, repeat) = match symbol {
                16 => {
                    if index == 0 {
                        return Err("repeat with no previous length".to_string());
                    }
                    (lengths[index - 1], 3 + bits.bits(2)? as usize)
                }
                17 => (0, 3 + bits.bits(3)? as usize),
                _ => (0, 11 + bits.bits(7)? as usize),
            };
            if index + repeat > lengths.len() {
                return Err("too many code lengths".to_string());
            }
            for slot in &mut lengths[index..index + repeat] {
                *slot = value;
            }
            index += repeat;
        }
        Ok((
            Huffman::new(&lengths[..literal_count]),
            Huffman::new(&lengths[literal_count..]),
        ))
    }

    fn fixed() -> (Huffman, Huffman) {
        let mut lengths = [0u8; 288];
        for (symbol, length) in lengths.iter_mut().enumerate() {
            *length = match symbol {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        (Huffman::new(&lengths), Huffman::new(&[5u8; 30]))
    }

    /// Données zlib (en-tête de deux octets, puis DEFLATE).
    pub fn zlib(data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 2 || data[0] & 0x0f != 8 {
            return Err("not zlib data".to_string());
        }
        let mut bits = Bits {
            data: &data[2..],
            pos: 0,
            buffer: 0,
            count: 0,
        };
        let mut out = Vec::new();
        loop {
            let last = bits.bits(1)?;
            match bits.bits(2)? {
                0 => {
                    bits.buffer = 0;
                    bits.count = 0;
                    let at = bits.pos;
                    let header = bits.data.get(at..at + 4).ok_or("stored block cut")?;
                    let length = usize::from(u16::from_le_bytes([header[0], header[1]]));
                    let block = bits
                        .data
                        .get(at + 4..at + 4 + length)
                        .ok_or("stored block cut")?;
                    out.extend_from_slice(block);
                    bits.pos = at + 4 + length;
                }
                1 => {
                    let (lengths, distances) = fixed();
                    codes(&mut bits, &mut out, &lengths, &distances)?;
                }
                2 => {
                    let (lengths, distances) = dynamic(&mut bits)?;
                    codes(&mut bits, &mut out, &lengths, &distances)?;
                }
                _ => return Err("invalid block type in compressed data".to_string()),
            }
            if last == 1 {
                return Ok(out);
            }
        }
    }
}

// ============================================================================ PNG

mod png {
    use super::{inflate, Pixels};

    fn over_white(value: u8, alpha: u8) -> u8 {
        let mixed =
            (u32::from(value) * u32::from(alpha) + 255 * (255 - u32::from(alpha)) + 127) / 255;
        mixed as u8
    }

    pub fn decode(bytes: &[u8]) -> Result<Pixels, String> {
        let mut at = 8;
        let (mut width, mut height, mut depth, mut color, mut interlace) =
            (0usize, 0usize, 0u8, 0u8, 0u8);
        let mut palette: Vec<[u8; 3]> = Vec::new();
        let mut palette_alpha: Vec<u8> = Vec::new();
        let mut key: Option<[u16; 3]> = None;
        let mut idat = Vec::new();
        while at + 8 <= bytes.len() {
            let length =
                u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
                    as usize;
            let kind = &bytes[at + 4..at + 8];
            let data = bytes.get(at + 8..at + 8 + length).ok_or("PNG chunk cut")?;
            match kind {
                b"IHDR" if data.len() >= 13 => {
                    width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
                    height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
                    depth = data[8];
                    color = data[9];
                    interlace = data[12];
                }
                b"PLTE" => {
                    palette = data
                        .chunks_exact(3)
                        .map(|rgb| [rgb[0], rgb[1], rgb[2]])
                        .collect()
                }
                b"tRNS" => match color {
                    3 => palette_alpha = data.to_vec(),
                    0 if data.len() >= 2 => {
                        let gray = u16::from_be_bytes([data[0], data[1]]);
                        key = Some([gray, gray, gray]);
                    }
                    2 if data.len() >= 6 => {
                        key = Some([
                            u16::from_be_bytes([data[0], data[1]]),
                            u16::from_be_bytes([data[2], data[3]]),
                            u16::from_be_bytes([data[4], data[5]]),
                        ]);
                    }
                    _ => {}
                },
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => break,
                _ => {}
            }
            at += 12 + length;
        }
        if width == 0 || height == 0 {
            return Err("PNG without a valid header".to_string());
        }
        if width.saturating_mul(height) > 60_000_000 {
            return Err(format!("PNG too large to decode here ({width}x{height})"));
        }
        if interlace != 0 {
            return Err("interlaced PNG cannot be decoded here".to_string());
        }
        let channels = match color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => return Err(format!("unknown PNG color type {color}")),
        };
        let bits_per_pixel = channels * usize::from(depth);
        let stride = (width * bits_per_pixel).div_ceil(8);
        let step = bits_per_pixel.div_ceil(8).max(1);
        let raw = inflate::zlib(&idat)?;
        if raw.len() < height * (stride + 1) {
            return Err("PNG image data is incomplete".to_string());
        }
        let mut rows = vec![0u8; height * stride];
        for y in 0..height {
            let filter = raw[y * (stride + 1)];
            let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
            for x in 0..stride {
                let left = if x >= step {
                    rows[y * stride + x - step]
                } else {
                    0
                };
                let up = if y > 0 { rows[(y - 1) * stride + x] } else { 0 };
                let corner = if y > 0 && x >= step {
                    rows[(y - 1) * stride + x - step]
                } else {
                    0
                };
                let value = line[x];
                rows[y * stride + x] = match filter {
                    0 => value,
                    1 => value.wrapping_add(left),
                    2 => value.wrapping_add(up),
                    3 => value.wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                    4 => {
                        let estimate = i16::from(left) + i16::from(up) - i16::from(corner);
                        let (pa, pb, pc) = (
                            (estimate - i16::from(left)).abs(),
                            (estimate - i16::from(up)).abs(),
                            (estimate - i16::from(corner)).abs(),
                        );
                        let predictor = if pa <= pb && pa <= pc {
                            left
                        } else if pb <= pc {
                            up
                        } else {
                            corner
                        };
                        value.wrapping_add(predictor)
                    }
                    _ => return Err(format!("unknown PNG filter {filter}")),
                };
            }
        }
        let sample = |row: &[u8], index: usize| -> u16 {
            match depth {
                16 => u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]),
                8 => u16::from(row[index]),
                _ => {
                    let bit = index * usize::from(depth);
                    let byte = row[bit / 8];
                    let shift = 8 - usize::from(depth) - bit % 8;
                    u16::from((byte >> shift) & ((1u8 << depth) - 1))
                }
            }
        };
        let to_byte = |value: u16| -> u8 {
            match depth {
                16 => (value >> 8) as u8,
                8 => value as u8,
                _ => (u32::from(value) * 255 / ((1u32 << depth) - 1)) as u8,
            }
        };
        let mut rgb = Vec::with_capacity(width * height);
        let mut transparent = false;
        for y in 0..height {
            let row = &rows[y * stride..(y + 1) * stride];
            for x in 0..width {
                let (pixel, alpha) = match color {
                    0 => {
                        let gray = sample(row, x);
                        let alpha = if key == Some([gray, gray, gray]) {
                            0
                        } else {
                            255
                        };
                        let gray = to_byte(gray);
                        ([gray; 3], alpha)
                    }
                    2 => {
                        let values = [
                            sample(row, x * 3),
                            sample(row, x * 3 + 1),
                            sample(row, x * 3 + 2),
                        ];
                        let alpha = if key == Some(values) { 0 } else { 255 };
                        (
                            [to_byte(values[0]), to_byte(values[1]), to_byte(values[2])],
                            alpha,
                        )
                    }
                    3 => {
                        let index = usize::from(sample(row, x));
                        let rgb = palette.get(index).copied().unwrap_or([0, 0, 0]);
                        (rgb, palette_alpha.get(index).copied().unwrap_or(255))
                    }
                    4 => {
                        let gray = to_byte(sample(row, x * 2));
                        ([gray; 3], to_byte(sample(row, x * 2 + 1)))
                    }
                    _ => (
                        [
                            to_byte(sample(row, x * 4)),
                            to_byte(sample(row, x * 4 + 1)),
                            to_byte(sample(row, x * 4 + 2)),
                        ],
                        to_byte(sample(row, x * 4 + 3)),
                    ),
                };
                if alpha < 255 {
                    transparent = true;
                    rgb.push([
                        over_white(pixel[0], alpha),
                        over_white(pixel[1], alpha),
                        over_white(pixel[2], alpha),
                    ]);
                } else {
                    rgb.push(pixel);
                }
            }
        }
        Ok(Pixels {
            width,
            height,
            rgb,
            transparent,
        })
    }
}

// ============================================================================ JPEG

mod jpeg {
    //! JPEG séquentiel de base (codage de Huffman), le format des photos et des captures.
    use super::Pixels;

    const ZIGZAG: [usize; 64] = [
        0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27,
        20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
        58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
    ];

    #[derive(Clone, Default)]
    struct Table {
        // Pour chaque longueur 1..=16 : premier code, nombre de codes, place du premier symbole.
        first: [i32; 17],
        count: [i32; 17],
        offset: [i32; 17],
        symbols: Vec<u8>,
    }

    struct Component {
        id: u8,
        h: usize,
        v: usize,
        quant: usize,
        dc: usize,
        ac: usize,
        prediction: i32,
        /// Plan des échantillons, largeur `stride`.
        plane: Vec<u8>,
        stride: usize,
        blocks_wide: usize,
        blocks_high: usize,
    }

    struct Reader<'a> {
        data: &'a [u8],
        pos: usize,
        buffer: u32,
        count: u32,
        /// Un marqueur a été atteint : la suite des données est faite de zéros.
        marker: bool,
    }

    impl Reader<'_> {
        fn fill(&mut self) {
            while self.count <= 24 {
                let mut byte = 0u32;
                if !self.marker && self.pos < self.data.len() {
                    let value = self.data[self.pos];
                    if value == 0xff {
                        let next = self.data.get(self.pos + 1).copied().unwrap_or(0);
                        if next == 0 {
                            self.pos += 2;
                            byte = 0xff;
                        } else {
                            self.marker = true;
                        }
                    } else {
                        self.pos += 1;
                        byte = u32::from(value);
                    }
                }
                self.buffer |= byte << (24 - self.count);
                self.count += 8;
            }
        }

        fn bit(&mut self) -> i32 {
            self.fill();
            let bit = (self.buffer >> 31) as i32;
            self.buffer <<= 1;
            self.count -= 1;
            bit
        }

        fn bits(&mut self, count: u32) -> i32 {
            if count == 0 {
                return 0;
            }
            self.fill();
            let value = (self.buffer >> (32 - count)) as i32;
            self.buffer <<= count;
            self.count -= count;
            value
        }

        fn decode(&mut self, table: &Table) -> Result<u8, String> {
            let mut code = 0i32;
            for length in 1..=16 {
                code = (code << 1) | self.bit();
                if table.count[length] > 0
                    && code - table.first[length] < table.count[length]
                    && code >= table.first[length]
                {
                    let index = table.offset[length] + code - table.first[length];
                    return table
                        .symbols
                        .get(index as usize)
                        .copied()
                        .ok_or_else(|| "bad Huffman table in JPEG".to_string());
                }
            }
            Err("invalid Huffman code in JPEG data".to_string())
        }

        /// Après un marqueur de reprise : repart à l'octet suivant le marqueur.
        fn restart(&mut self) {
            self.buffer = 0;
            self.count = 0;
            self.marker = false;
            while self.pos + 1 < self.data.len() {
                if self.data[self.pos] == 0xff && (0xd0..=0xd7).contains(&self.data[self.pos + 1]) {
                    self.pos += 2;
                    return;
                }
                self.pos += 1;
            }
        }
    }

    fn extend(value: i32, size: u32) -> i32 {
        if size == 0 {
            0
        } else if value < (1 << (size - 1)) {
            value - (1 << size) + 1
        } else {
            value
        }
    }

    fn idct_table() -> [[f32; 8]; 8] {
        let mut table = [[0f32; 8]; 8];
        for (x, row) in table.iter_mut().enumerate() {
            for (u, cell) in row.iter_mut().enumerate() {
                let scale = if u == 0 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    1.0
                };
                *cell = scale
                    * ((2.0 * x as f32 + 1.0) * u as f32 * std::f32::consts::PI / 16.0).cos()
                    / 2.0;
            }
        }
        table
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_block(
        reader: &mut Reader,
        component: &mut Component,
        dc: &Table,
        ac: &Table,
        quant: &[i32; 64],
        cosines: &[[f32; 8]; 8],
        block_x: usize,
        block_y: usize,
    ) -> Result<(), String> {
        let mut coefficients = [0i32; 64];
        let size = u32::from(reader.decode(dc)?);
        let diff = extend(reader.bits(size), size);
        component.prediction += diff;
        coefficients[0] = component.prediction * quant[0];
        let mut index = 1;
        while index < 64 {
            let symbol = reader.decode(ac)?;
            let (run, size) = (usize::from(symbol >> 4), u32::from(symbol & 15));
            if size == 0 {
                if run == 15 {
                    index += 16;
                    continue;
                }
                break;
            }
            index += run;
            if index > 63 {
                break;
            }
            coefficients[ZIGZAG[index]] = extend(reader.bits(size), size) * quant[index];
            index += 1;
        }
        // Transformée inverse, ligne puis colonne.
        let mut temp = [0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                let mut sum = 0f32;
                for u in 0..8 {
                    let value = coefficients[y * 8 + u];
                    if value != 0 {
                        sum += cosines[x][u] * value as f32;
                    }
                }
                temp[y * 8 + x] = sum;
            }
        }
        for x in 0..8 {
            for y in 0..8 {
                let mut sum = 0f32;
                for v in 0..8 {
                    sum += cosines[y][v] * temp[v * 8 + x];
                }
                let value = (sum + 128.0).round().clamp(0.0, 255.0) as u8;
                let px = block_x * 8 + x;
                let py = block_y * 8 + y;
                if px < component.stride && py * component.stride + px < component.plane.len() {
                    component.plane[py * component.stride + px] = value;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn decode(bytes: &[u8]) -> Result<Pixels, String> {
        let mut quant = [[0i32; 64]; 4];
        let mut dc_tables: Vec<Table> = vec![Table::default(); 4];
        let mut ac_tables: Vec<Table> = vec![Table::default(); 4];
        let mut components: Vec<Component> = Vec::new();
        let (mut width, mut height) = (0usize, 0usize);
        let (mut hmax, mut vmax) = (1usize, 1usize);
        let mut restart_interval = 0usize;
        let mut adobe_rgb = false;
        let cosines = idct_table();
        let mut at = 2;
        while at + 4 <= bytes.len() {
            if bytes[at] != 0xff {
                at += 1;
                continue;
            }
            let marker = bytes[at + 1];
            if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 || marker == 0xff
            {
                at += if marker == 0xff { 1 } else { 2 };
                continue;
            }
            if marker == 0xd9 {
                break;
            }
            let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
            let data = bytes
                .get(at + 4..at + 2 + length)
                .ok_or("JPEG segment cut")?;
            match marker {
                0xdb => {
                    let mut pos = 0;
                    while pos < data.len() {
                        let precision = data[pos] >> 4;
                        let id = usize::from(data[pos] & 3);
                        pos += 1;
                        for index in 0..64 {
                            quant[id][index] = if precision == 0 {
                                i32::from(*data.get(pos + index).ok_or("JPEG table cut")?)
                            } else {
                                i32::from(u16::from_be_bytes([
                                    *data.get(pos + index * 2).ok_or("JPEG table cut")?,
                                    *data.get(pos + index * 2 + 1).ok_or("JPEG table cut")?,
                                ]))
                            };
                        }
                        pos += if precision == 0 { 64 } else { 128 };
                    }
                }
                0xc0 | 0xc1 => {
                    if data.len() < 6 {
                        return Err("JPEG frame header cut".to_string());
                    }
                    height = usize::from(u16::from_be_bytes([data[1], data[2]]));
                    width = usize::from(u16::from_be_bytes([data[3], data[4]]));
                    let count = usize::from(data[5]);
                    for index in 0..count {
                        let base = 6 + index * 3;
                        let part = data.get(base..base + 3).ok_or("JPEG frame header cut")?;
                        components.push(Component {
                            id: part[0],
                            h: usize::from(part[1] >> 4).max(1),
                            v: usize::from(part[1] & 15).max(1),
                            quant: usize::from(part[2] & 3),
                            dc: 0,
                            ac: 0,
                            prediction: 0,
                            plane: Vec::new(),
                            stride: 0,
                            blocks_wide: 0,
                            blocks_high: 0,
                        });
                    }
                    if width == 0 || height == 0 {
                        return Err("JPEG without dimensions".to_string());
                    }
                    if width.saturating_mul(height) > 60_000_000 {
                        return Err(format!("JPEG too large to decode here ({width}x{height})"));
                    }
                    hmax = components.iter().map(|c| c.h).max().unwrap_or(1);
                    vmax = components.iter().map(|c| c.v).max().unwrap_or(1);
                    let mcus_wide = width.div_ceil(8 * hmax);
                    let mcus_high = height.div_ceil(8 * vmax);
                    for component in &mut components {
                        component.blocks_wide = mcus_wide * component.h;
                        component.blocks_high = mcus_high * component.v;
                        component.stride = component.blocks_wide * 8;
                        component.plane = vec![0u8; component.stride * component.blocks_high * 8];
                    }
                }
                0xc2 | 0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                    return Err("progressive or arithmetic JPEG cannot be decoded here".to_string());
                }
                0xc4 => {
                    let mut pos = 0;
                    while pos + 17 <= data.len() {
                        let class = data[pos] >> 4;
                        let id = usize::from(data[pos] & 3);
                        let mut table = Table::default();
                        let mut code = 0i32;
                        let mut offset = 0i32;
                        for length in 1..=16 {
                            let count = i32::from(data[pos + length]);
                            table.first[length] = code;
                            table.count[length] = count;
                            table.offset[length] = offset;
                            code = (code + count) << 1;
                            offset += count;
                        }
                        let total = offset as usize;
                        table.symbols = data
                            .get(pos + 17..pos + 17 + total)
                            .ok_or("JPEG Huffman table cut")?
                            .to_vec();
                        pos += 17 + total;
                        if class == 0 {
                            dc_tables[id] = table;
                        } else {
                            ac_tables[id] = table;
                        }
                    }
                }
                0xdd if data.len() >= 2 => {
                    restart_interval = usize::from(u16::from_be_bytes([data[0], data[1]]));
                }
                0xee if data.starts_with(b"Adobe") && data.len() >= 12 => {
                    adobe_rgb = data[11] == 0;
                }
                0xda => {
                    if components.is_empty() {
                        return Err("JPEG scan before its frame header".to_string());
                    }
                    let count = usize::from(data[0]);
                    let mut chosen = Vec::new();
                    for index in 0..count {
                        let id = data[1 + index * 2];
                        let tables = data[2 + index * 2];
                        let position = components
                            .iter()
                            .position(|c| c.id == id)
                            .ok_or("JPEG scan names an unknown component")?;
                        components[position].dc = usize::from(tables >> 4) & 3;
                        components[position].ac = usize::from(tables & 15) & 3;
                        chosen.push(position);
                    }
                    for component in &mut components {
                        component.prediction = 0;
                    }
                    let mut reader = Reader {
                        data: bytes,
                        pos: at + 2 + length,
                        buffer: 0,
                        count: 0,
                        marker: false,
                    };
                    let mut done = 0usize;
                    let mut step = |reader: &mut Reader, components: &mut Vec<Component>| {
                        if restart_interval > 0 && done > 0 && done % restart_interval == 0 {
                            reader.restart();
                            for component in components.iter_mut() {
                                component.prediction = 0;
                            }
                        }
                        done += 1;
                    };
                    if chosen.len() == 1 {
                        let index = chosen[0];
                        let wide = (width * components[index].h).div_ceil(8 * hmax);
                        let high = (height * components[index].v).div_ceil(8 * vmax);
                        for block_y in 0..high {
                            for block_x in 0..wide {
                                step(&mut reader, &mut components);
                                let (dc, ac, q) = (
                                    components[index].dc,
                                    components[index].ac,
                                    components[index].quant,
                                );
                                decode_block(
                                    &mut reader,
                                    &mut components[index],
                                    &dc_tables[dc],
                                    &ac_tables[ac],
                                    &quant[q],
                                    &cosines,
                                    block_x,
                                    block_y,
                                )?;
                            }
                        }
                    } else {
                        let mcus_wide = width.div_ceil(8 * hmax);
                        let mcus_high = height.div_ceil(8 * vmax);
                        for mcu_y in 0..mcus_high {
                            for mcu_x in 0..mcus_wide {
                                step(&mut reader, &mut components);
                                for &index in &chosen {
                                    let (h, v) = (components[index].h, components[index].v);
                                    let (dc, ac, q) = (
                                        components[index].dc,
                                        components[index].ac,
                                        components[index].quant,
                                    );
                                    for by in 0..v {
                                        for bx in 0..h {
                                            decode_block(
                                                &mut reader,
                                                &mut components[index],
                                                &dc_tables[dc],
                                                &ac_tables[ac],
                                                &quant[q],
                                                &cosines,
                                                mcu_x * h + bx,
                                                mcu_y * v + by,
                                            )?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Suite du fichier : après les données du balayage.
                    let mut next = reader.pos;
                    while next + 1 < bytes.len()
                        && !(bytes[next] == 0xff
                            && bytes[next + 1] != 0
                            && !(0xd0..=0xd7).contains(&bytes[next + 1]))
                    {
                        next += 1;
                    }
                    at = next;
                    continue;
                }
                _ => {}
            }
            at += 2 + length;
        }
        if components.is_empty() || width == 0 {
            return Err("JPEG without image data".to_string());
        }
        let sample = |component: &Component, x: usize, y: usize| -> f32 {
            let sx = x * component.h / hmax;
            let sy = y * component.v / vmax;
            f32::from(component.plane[sy * component.stride + sx])
        };
        let mut rgb = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let pixel = match components.len() {
                    1 | 2 => {
                        let gray = sample(&components[0], x, y) as u8;
                        [gray; 3]
                    }
                    3 if adobe_rgb => [
                        sample(&components[0], x, y) as u8,
                        sample(&components[1], x, y) as u8,
                        sample(&components[2], x, y) as u8,
                    ],
                    3 => {
                        let luma = sample(&components[0], x, y);
                        let cb = sample(&components[1], x, y) - 128.0;
                        let cr = sample(&components[2], x, y) - 128.0;
                        [
                            (luma + 1.402 * cr).round().clamp(0.0, 255.0) as u8,
                            (luma - 0.344_136 * cb - 0.714_136 * cr)
                                .round()
                                .clamp(0.0, 255.0) as u8,
                            (luma + 1.772 * cb).round().clamp(0.0, 255.0) as u8,
                        ]
                    }
                    _ => return Err("CMYK JPEG cannot be decoded here".to_string()),
                };
                rgb.push(pixel);
            }
        }
        Ok(Pixels {
            width,
            height,
            rgb,
            transparent: false,
        })
    }
}

// ============================================================================ rendu

/// Lettres de la palette, la plus présente d'abord.
const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOP";
/// Niveaux du dessin en caractères, du plus sombre au plus clair.
const SHADES: &[u8] = b"@%#*+=-:. ";
const MAX_COLUMNS: usize = 64;

/// Nom approximatif d'une couleur, pour un modèle qui ne voit pas les codes.
pub fn color_name(rgb: [u8; 3]) -> &'static str {
    let [r, g, b] = rgb.map(|value| f32::from(value) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let saturation = if max == min {
        0.0
    } else {
        (max - min) / (1.0 - (2.0 * lightness - 1.0).abs())
    };
    if saturation < 0.15 || max - min < 0.08 {
        return match lightness {
            l if l < 0.12 => "black",
            l if l < 0.3 => "very dark gray",
            l if l < 0.5 => "dark gray",
            l if l < 0.7 => "gray",
            l if l < 0.9 => "light gray",
            _ => "white",
        };
    }
    let hue = if max == r {
        60.0 * (((g - b) / (max - min)).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / (max - min) + 2.0)
    } else {
        60.0 * ((r - g) / (max - min) + 4.0)
    };
    if lightness < 0.18 {
        return "very dark (almost black)";
    }
    if lightness > 0.9 {
        return "very light (almost white)";
    }
    let brown = (15.0..45.0).contains(&hue) && lightness < 0.45;
    match hue {
        _ if brown => "brown",
        h if !(15.0..345.0).contains(&h) => {
            if lightness > 0.7 {
                "pink"
            } else {
                "red"
            }
        }
        h if h < 45.0 => "orange",
        h if h < 70.0 => "yellow",
        h if h < 165.0 => "green",
        h if h < 200.0 => "cyan",
        h if h < 255.0 => "blue",
        h if h < 290.0 => "purple",
        _ => "pink",
    }
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    let [dr, dg, db] = [
        i32::from(a[0]) - i32::from(b[0]),
        i32::from(a[1]) - i32::from(b[1]),
        i32::from(a[2]) - i32::from(b[2]),
    ];
    // Pondération perceptive simple (le vert compte plus).
    2 * dr * dr + 4 * dg * dg + 3 * db * db
}

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Couleurs dominantes : les pixels regroupés par couleur proche, au plus 16, avec leur part.
fn palette(pixels: &Pixels) -> Vec<([u8; 3], usize)> {
    // Un pixel sur `skip` suffit pour une grande image.
    let skip = (pixels.rgb.len() / 400_000).max(1);
    let mut buckets: std::collections::HashMap<[u8; 3], (usize, [u64; 3])> =
        std::collections::HashMap::new();
    for pixel in pixels.rgb.iter().step_by(skip) {
        let key = pixel.map(|value| value >> 3);
        let entry = buckets.entry(key).or_insert((0, [0; 3]));
        entry.0 += 1;
        for channel in 0..3 {
            entry.1[channel] += u64::from(pixel[channel]);
        }
    }
    let mut sorted: Vec<([u8; 3], usize)> = buckets
        .into_values()
        .map(|(count, sums)| (sums.map(|sum| (sum / count as u64) as u8), count))
        .collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let mut chosen: Vec<([u8; 3], usize)> = Vec::new();
    for (color, count) in sorted {
        let close = chosen
            .iter()
            .position(|(kept, _)| distance(*kept, color) < 450);
        if let Some(index) = close {
            chosen[index].1 += count;
        } else if chosen.len() < LETTERS.len() {
            chosen.push((color, count));
        } else if let Some(index) = chosen
            .iter()
            .enumerate()
            .min_by_key(|(_, (kept, _))| distance(*kept, color))
            .map(|(index, _)| index)
        {
            chosen[index].1 += count;
        }
    }
    // Couleurs presque absentes (anticrénelage, bruit) : versées dans la plus proche des autres.
    let total: usize = chosen.iter().map(|(_, count)| count).sum();
    let (kept, dropped): (Vec<_>, Vec<_>) = chosen
        .into_iter()
        .partition(|(_, count)| count * 1000 >= total);
    let mut kept = if kept.is_empty() {
        dropped.clone()
    } else {
        kept
    };
    for (color, count) in dropped {
        if let Some(index) = kept
            .iter()
            .enumerate()
            .min_by_key(|(_, (other, _))| distance(*other, color))
            .map(|(index, _)| index)
        {
            if kept[index].0 != color {
                kept[index].1 += count;
            }
        }
    }
    kept.sort_by(|a, b| b.1.cmp(&a.1));
    kept
}

fn nearest(palette: &[([u8; 3], usize)], color: [u8; 3]) -> usize {
    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, (kept, _))| distance(*kept, color))
        .map_or(0, |(index, _)| index)
}

/// Grille : pour chaque case, la couleur de la palette qui en couvre le plus, et sa luminosité
/// moyenne.
struct Grid {
    columns: usize,
    rows: usize,
    colors: Vec<usize>,
    light: Vec<f32>,
}

fn grid(pixels: &Pixels, palette: &[([u8; 3], usize)]) -> Grid {
    let columns = pixels.width.min(MAX_COLUMNS).max(1);
    // Un caractère est deux fois plus haut que large.
    let rows = ((pixels.height * columns) as f32 / pixels.width as f32 / 2.0)
        .round()
        .clamp(1.0, 100.0) as usize;
    let mut colors = Vec::with_capacity(columns * rows);
    let mut light = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        let y0 = row * pixels.height / rows;
        let y1 = ((row + 1) * pixels.height / rows).max(y0 + 1);
        for column in 0..columns {
            let x0 = column * pixels.width / columns;
            let x1 = ((column + 1) * pixels.width / columns).max(x0 + 1);
            let step_x = ((x1 - x0) / 8).max(1);
            let step_y = ((y1 - y0) / 8).max(1);
            let mut votes = vec![0usize; palette.len().max(1)];
            let (mut total, mut count) = (0f32, 0f32);
            for y in (y0..y1).step_by(step_y) {
                for x in (x0..x1).step_by(step_x) {
                    let pixel = pixels.rgb[y * pixels.width + x];
                    votes[nearest(palette, pixel)] += 1;
                    total += 0.2126 * f32::from(pixel[0])
                        + 0.7152 * f32::from(pixel[1])
                        + 0.0722 * f32::from(pixel[2]);
                    count += 1.0;
                }
            }
            let winner = votes
                .iter()
                .enumerate()
                .max_by_key(|(_, votes)| **votes)
                .map_or(0, |(index, _)| index);
            colors.push(winner);
            light.push(if count > 0.0 { total / count } else { 0.0 });
        }
    }
    Grid {
        columns,
        rows,
        colors,
        light,
    }
}

/// Zones : cases voisines de la même couleur, avec leur cadre.
struct Region {
    color: usize,
    cells: usize,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

fn regions(grid: &Grid) -> Vec<Region> {
    let mut seen = vec![false; grid.colors.len()];
    let mut found = Vec::new();
    for start in 0..grid.colors.len() {
        if seen[start] {
            continue;
        }
        let color = grid.colors[start];
        let mut stack = vec![start];
        seen[start] = true;
        let mut region = Region {
            color,
            cells: 0,
            left: usize::MAX,
            top: usize::MAX,
            right: 0,
            bottom: 0,
        };
        while let Some(index) = stack.pop() {
            let (x, y) = (index % grid.columns, index / grid.columns);
            region.cells += 1;
            region.left = region.left.min(x);
            region.right = region.right.max(x);
            region.top = region.top.min(y);
            region.bottom = region.bottom.max(y);
            let mut push = |next: usize| {
                if !seen[next] && grid.colors[next] == color {
                    seen[next] = true;
                    stack.push(next);
                }
            };
            if x > 0 {
                push(index - 1);
            }
            if x + 1 < grid.columns {
                push(index + 1);
            }
            if y > 0 {
                push(index - grid.columns);
            }
            if y + 1 < grid.rows {
                push(index + grid.columns);
            }
        }
        found.push(region);
    }
    found.sort_by(|a, b| b.cells.cmp(&a.cells));
    found
}

/// Le texte qui représente l'image, et son SVG simplifié.
pub fn render(pixels: &Pixels, format: &str) -> (String, String) {
    let palette = palette(pixels);
    let total: usize = palette.iter().map(|(_, count)| count).sum::<usize>().max(1);
    let grid = grid(pixels, &palette);
    let cell_w = pixels.width as f32 / grid.columns as f32;
    let cell_h = pixels.height as f32 / grid.rows as f32;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "IMAGE: {} x {} pixels, {format}; orientation {}. Computed from the pixels by Claw (exact, nothing guessed).",
        pixels.width,
        pixels.height,
        if pixels.width > pixels.height { "landscape" } else if pixels.width < pixels.height { "portrait" } else { "square" }
    );
    if pixels.transparent {
        text.push_str("Transparent areas are shown as white.\n");
    }
    let _ = writeln!(
        text,
        "Grid below: {} columns x {} rows; one cell = about {:.0} x {:.0} pixels (column c covers x = c*{:.0}..; row r covers y = r*{:.0}..).",
        grid.columns, grid.rows, cell_w, cell_h, cell_w, cell_h
    );
    text.push_str("\nPALETTE (letter, color, name, share of the image):\n");
    for (index, (color, count)) in palette.iter().enumerate() {
        let _ = writeln!(
            text,
            "  {} {} {:<26} {:>5.1}%",
            LETTERS[index] as char,
            hex(*color),
            color_name(*color),
            *count as f32 * 100.0 / total as f32
        );
    }
    // Repères des colonnes, de 10 en 10.
    let ruler: String = (0..grid.columns)
        .map(|column| {
            if column % 10 == 0 {
                char::from_digit((column / 10 % 10) as u32, 10).unwrap_or('?')
            } else {
                ' '
            }
        })
        .collect();
    text.push_str("\nCOLOR GRID (each letter = the palette color covering most of that cell):\n");
    let _ = writeln!(text, "     {ruler}");
    for row in 0..grid.rows {
        let line: String = grid.colors[row * grid.columns..(row + 1) * grid.columns]
            .iter()
            .map(|&index| LETTERS[index] as char)
            .collect();
        let _ = writeln!(text, "  {row:>2} {line}");
    }
    text.push_str("\nREGIONS (connected areas of one color, largest first; box as a share of the width and height, then in pixels):\n");
    let cells = grid.colors.len();
    let mut listed = 0;
    for region in regions(&grid) {
        if listed >= 30 || region.cells * 200 < cells {
            break;
        }
        listed += 1;
        let (color, _) = palette[region.color];
        let _ = writeln!(
            text,
            "  {} {} {}: x {:.0}-{:.0}%, y {:.0}-{:.0}% (x {}-{}, y {}-{} px), {:.1}% of the image",
            LETTERS[region.color] as char,
            hex(color),
            color_name(color),
            region.left as f32 * 100.0 / grid.columns as f32,
            (region.right + 1) as f32 * 100.0 / grid.columns as f32,
            region.top as f32 * 100.0 / grid.rows as f32,
            (region.bottom + 1) as f32 * 100.0 / grid.rows as f32,
            (region.left as f32 * cell_w) as usize,
            (((region.right + 1) as f32 * cell_w) as usize).min(pixels.width),
            (region.top as f32 * cell_h) as usize,
            (((region.bottom + 1) as f32 * cell_h) as usize).min(pixels.height),
            region.cells as f32 * 100.0 / cells as f32,
        );
    }
    if listed == 0 {
        text.push_str("  none larger than half a percent\n");
    }
    // Dessin de la luminosité : les contrastes sont étirés pour que les formes se voient.
    let (low, high) = grid
        .light
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &value| {
            (low.min(value), high.max(value))
        });
    let span = (high - low).max(1.0);
    text.push_str("\nSHAPES (brightness, darkest '@' to lightest ' '; text lines show as rows of darker marks on a light background, or the reverse):\n");
    let _ = writeln!(text, "     {ruler}");
    for row in 0..grid.rows {
        let line: String = grid.light[row * grid.columns..(row + 1) * grid.columns]
            .iter()
            .map(|&value| {
                let level = (((value - low) / span) * (SHADES.len() - 1) as f32).round() as usize;
                SHADES[level.min(SHADES.len() - 1)] as char
            })
            .collect();
        let _ = writeln!(text, "  {row:>2} {}", line.trim_end());
    }
    // SVG : chaque suite de cases de même couleur sur une ligne devient un rectangle.
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">",
        w = pixels.width,
        h = pixels.height
    );
    let _ = writeln!(
        svg,
        "  <!-- Simplified from the pixels by Claw: {} x {} cells, {} colors. -->",
        grid.columns,
        grid.rows,
        palette.len()
    );
    let _ = writeln!(
        svg,
        "  <rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",
        hex(palette[0].0)
    );
    for row in 0..grid.rows {
        let y = (row as f32 * cell_h).round() as usize;
        let height = (((row + 1) as f32 * cell_h).round() as usize).saturating_sub(y);
        let mut column = 0;
        while column < grid.columns {
            let color = grid.colors[row * grid.columns + column];
            let start = column;
            while column < grid.columns && grid.colors[row * grid.columns + column] == color {
                column += 1;
            }
            if color == 0 {
                continue;
            }
            let x = (start as f32 * cell_w).round() as usize;
            let width = ((column as f32 * cell_w).round() as usize).saturating_sub(x);
            let _ = writeln!(
                svg,
                "  <rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" fill=\"{}\"/>",
                hex(palette[color].0)
            );
        }
    }
    svg.push_str("</svg>\n");
    (text, svg)
}

/// Texte et SVG d'une image, ou pourquoi elle n'a pas pu être lue.
pub fn represent(bytes: &[u8]) -> Result<(String, String), String> {
    let format = format_of(bytes);
    let pixels = decode(bytes)?;
    Ok(render(&pixels, format))
}

#[cfg(test)]
mod tests {
    use super::{color_name, decode, format_of, represent};

    // 40 x 20 : bandeau presque noir en haut (5 px), carré vert en bas à droite, fond blanc.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAACgAAAAUCAIAAABwJOjsAAAAOUlEQVR4nGOUlpJgGAjANCC2jlpMT8D4////AbF45AX1yLOYhRLNBt3eZOsdeUE9avGoxaMWUw0AAGJvBHkEBLejAAAAAElFTkSuQmCC";
    const JPEG: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAIBAQEBAQIBAQECAgICAgQDAgICAgUEBAMEBgUGBgYFBgYGBwkIBgcJBwYGCAsICQoKCgoKBggLDAsKDAkKCgr/2wBDAQICAgICAgUDAwUKBwYHCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgr/wAARCAAUACgDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDz7/iEA/4KV/8ARcPgZ/4Uus//ACpo/wCIQD/gpX/0XD4Gf+FLrP8A8qa/o9opcqHzM/nC/wCIQD/gpX/0XD4Gf+FLrP8A8qaP+IQD/gpX/wBFw+Bn/hS6z/8AKmv1Z/4LO/th/tGfsnf8K2/4UB8RP7A/t/8Atj+1v+JRZ3Xn+R9h8r/j5hk27fOk+7jO7nOBj4b/AOHw3/BRn/o4n/y0dI/+RK8zEZphsNWdOSd12t69z5LM+NMsynHTwtaE3KNr2UbapPrJPZ9jwr/iEA/4KV/9Fw+Bn/hS6z/8qaK91/4fDf8ABRn/AKOJ/wDLR0j/AORKKw/tvB/yy+5f5nB/xETJv+fdT7o//Jn7kUUUV7Z92fmf/wAHE/8AzR7/ALmD/wBxtfmfRRXxmbf8jCfy/JH4Pxn/AMlLX/7d/wDSIhRRRXnHy5//2Q==";

    fn bytes(text: &str) -> Vec<u8> {
        runtime::attachments::decode_base64(text).expect("base64")
    }

    #[test]
    fn png_and_jpeg_pixels_are_decoded() {
        let png = decode(&bytes(PNG)).expect("png");
        assert_eq!((png.width, png.height), (40, 20));
        assert_eq!(png.rgb[0], [0x1b, 0x1a, 0x18]);
        assert_eq!(png.rgb[19 * 40 + 39], [0x2f, 0x8a, 0x4a]);
        assert_eq!(png.rgb[10 * 40 + 5], [255, 255, 255]);
        let jpeg = decode(&bytes(JPEG)).expect("jpeg");
        assert_eq!((jpeg.width, jpeg.height), (40, 20));
        let near = |pixel: [u8; 3], want: [u8; 3]| {
            pixel
                .iter()
                .zip(want)
                .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 12)
        };
        assert!(
            near(jpeg.rgb[40 + 10], [0x1b, 0x1a, 0x18]),
            "{:?}",
            jpeg.rgb[40 + 10]
        );
        assert!(
            near(jpeg.rgb[16 * 40 + 30], [0x2f, 0x8a, 0x4a]),
            "{:?}",
            jpeg.rgb[16 * 40 + 30]
        );
        assert_eq!(format_of(b"GIF89a..."), "gif");
        assert!(decode(b"GIF89a...").is_err());
    }

    #[test]
    fn the_text_gives_palette_regions_and_shapes() {
        let (text, svg) = represent(&bytes(PNG)).expect("rendu");
        assert!(
            text.starts_with("IMAGE: 40 x 20 pixels, png; orientation landscape."),
            "{text}"
        );
        assert!(text.contains("#ffffff white"), "{text}");
        assert!(text.contains("#2f8a4a green"), "{text}");
        // Le carré vert : moitié droite, moitié basse.
        assert!(
            text.contains("green: x 50-100%, y 50-100% (x 20-40, y 10-20 px)"),
            "{text}"
        );
        assert!(text.contains("SHAPES"));
        assert!(
            svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\"")
        );
        assert!(svg.contains("fill=\"#2f8a4a\""));
        assert_eq!(color_name([0xc1, 0x3f, 0x2b]), "red");
        assert_eq!(color_name([0x26, 0x24, 0x21]), "very dark gray");
    }
}
