use snafu::{ResultExt, Snafu};
use std::{collections::HashMap, env, fs, io, io::Read};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("I/O error: {}", source))]
    InputOutput { source: std::io::Error },

    #[snafu(display("Unable to JSON encode: {}", source))]
    JsonEnc { source: serde_json::Error },

    #[snafu(display("Unable to process: {}", source))]
    Image { source: imgroll::Error },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

fn main() -> Result<()> {
    match &env::args().skip(1).collect::<Vec<String>>()[..] {
        [] => println!("use with paths or -"),
        [x] if x == "-" => {
            let mut buf = Vec::new();
            {
                let stdin_ = io::stdin();
                let mut stdin = stdin_.lock();
                stdin.read_to_end(&mut buf).context(InputOutput {})?;
            }
            output(imgroll::process_photo(&buf, "stdin").context(Image {})?)?;
        }
        paths => {
            for path in paths {
                let mut file = fs::File::open(path).context(InputOutput {})?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).context(InputOutput {})?;
                output(imgroll::process_photo(&buf, path).context(Image {})?)?;
            }
        }
    }

    Ok(())
}

fn output((photo, files): (imgroll::Photo, HashMap<String, Vec<u8>>)) -> Result<()> {
    println!("{}", serde_json::to_string(&photo).context(JsonEnc {})?);
    for (path, bytes) in files {
        use std::io::Write;
        let mut file = fs::File::create(path).context(InputOutput {})?;
        file.write_all(&bytes).context(InputOutput {})?;
    }
    Ok(())
}
