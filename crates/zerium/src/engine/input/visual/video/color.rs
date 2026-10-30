//! Source transfer functions and primaries normalized to linear BT.709.
use gstreamer::glib::translate::IntoGlib as _;
use gstreamer_video::{
    self as video, VideoColorPrimaries as Primaries, VideoTransferFunction as Transfer,
};

const TRANSFER_SAMPLES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VideoColor {
    transfer: Transfer,
    primaries: Primaries,
}

impl VideoColor {
    pub(super) fn from_info(info: &video::VideoInfo) -> Self {
        let color = info.colorimetry();
        let rgb = color.matrix() == video::VideoColorMatrix::Rgb;
        Self {
            transfer: match color.transfer() {
                Transfer::Unknown | Transfer::__Unknown(_) if rgb => Transfer::Srgb,
                Transfer::Unknown | Transfer::__Unknown(_) => Transfer::Bt709,
                transfer => transfer,
            },
            primaries: match color.primaries() {
                Primaries::Unknown | Primaries::__Unknown(_) => match color.matrix() {
                    video::VideoColorMatrix::Bt2020 => Primaries::Bt2020,
                    video::VideoColorMatrix::Bt601 if info.height() > 525 => Primaries::Bt470bg,
                    video::VideoColorMatrix::Bt601 => Primaries::Smpte170m,
                    _ => Primaries::Bt709,
                },
                primaries => primaries,
            },
        }
    }

    pub(crate) fn transfer_lut(self) -> Vec<f32> {
        // PQ is absolute; scene white and the export transform use 203 cd/m².
        let scale = if self.transfer == Transfer::Smpte2084 {
            10000. / 203.
        } else {
            1.
        };
        (0..TRANSFER_SAMPLES)
            .map(|index| {
                (self
                    .transfer
                    .decode(index as f64 / (TRANSFER_SAMPLES - 1) as f64)
                    * scale) as f32
            })
            .collect()
    }

    pub(crate) fn conversion(self) -> ColorConversion {
        let (source, white) = rgb_to_xyz(self.primaries);
        let (destination, destination_white) = rgb_to_xyz(Primaries::Bt709);
        let adaptation = white_adaptation(white, destination_white);
        let matrix = multiply(inverse(destination), multiply(adaptation, source));
        ColorConversion {
            red: [
                matrix[0][0] as f32,
                matrix[0][1] as f32,
                matrix[0][2] as f32,
                0.,
            ],
            green: [
                matrix[1][0] as f32,
                matrix[1][1] as f32,
                matrix[1][2] as f32,
                0.,
            ],
            blue: [
                matrix[2][0] as f32,
                matrix[2][1] as f32,
                matrix[2][2] as f32,
                0.,
            ],
            // HLG's inverse OETF is in the LUT. Apply the reference display OOTF
            // (1000 cd/m², gamma 1.2) before transforming its native primaries.
            luminance: [
                source[1][0] as f32,
                source[1][1] as f32,
                source[1][2] as f32,
                if self.transfer == Transfer::AribStdB67 {
                    1.
                } else {
                    0.
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ColorConversion {
    pub red: [f32; 4],
    pub green: [f32; 4],
    pub blue: [f32; 4],
    pub luminance: [f32; 4],
}

type Matrix = [[f64; 3]; 3];

fn transform(matrix: Matrix, value: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row.into_iter().zip(value).map(|(a, b)| a * b).sum())
}

fn multiply(left: Matrix, right: Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|i| left[row][i] * right[i][column]).sum())
    })
}

fn inverse(m: Matrix) -> Matrix {
    let cofactor = |row: usize, column: usize| {
        let a = (row + 1) % 3;
        let b = (row + 2) % 3;
        let c = (column + 1) % 3;
        let d = (column + 2) % 3;
        m[a][c] * m[b][d] - m[a][d] * m[b][c]
    };
    let determinant: f64 = (0..3).map(|i| m[0][i] * cofactor(0, i)).sum();
    std::array::from_fn(|row| std::array::from_fn(|column| cofactor(column, row) / determinant))
}

fn rgb_to_xyz(primaries: Primaries) -> (Matrix, [f64; 3]) {
    if primaries == Primaries::Smptest428 {
        // These channels already are XYZ. GStreamer leaves their reference
        // white undefined; bypass RGB scaling and chromatic adaptation.
        let (_, white) = rgb_to_xyz(Primaries::Bt709);
        return ([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], white);
    }
    // SAFETY: GStreamer returns immutable process-lifetime chromaticity tables.
    // Unknown future enum values fall back to the working primaries.
    let info = unsafe {
        video::ffi::gst_video_color_primaries_get_info(primaries.into_glib())
            .as_ref()
            .unwrap_or_else(|| {
                &*video::ffi::gst_video_color_primaries_get_info(Primaries::Bt709.into_glib())
            })
    };
    let xyz = |x: f64, y: f64| [x / y, 1., (1. - x - y) / y];
    let white = xyz(info.Wx, info.Wy);
    let columns = [
        [info.Rx, info.Ry, 1. - info.Rx - info.Ry],
        [info.Gx, info.Gy, 1. - info.Gx - info.Gy],
        [info.Bx, info.By, 1. - info.Bx - info.By],
    ];
    let matrix = std::array::from_fn(|row| std::array::from_fn(|column| columns[column][row]));
    let scales = transform(inverse(matrix), white);
    (
        std::array::from_fn(|row| {
            std::array::from_fn(|column| matrix[row][column] * scales[column])
        }),
        white,
    )
}

fn white_adaptation(source: [f64; 3], destination: [f64; 3]) -> Matrix {
    // Bradford adaptation, including sources whose reference white is not D65.
    let bradford = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let source = transform(bradford, source);
    let destination = transform(bradford, destination);
    let scaled = std::array::from_fn(|row| {
        bradford[row].map(|value| value * destination[row] / source[row])
    });
    multiply(inverse(bradford), scaled)
}
