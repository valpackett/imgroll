use aws_lambda_events::event::s3::S3Event;
use lambda_runtime::{self as lambda, error::HandlerError, lambda};
use rusoto_core::{Region, RusotoError};
use rusoto_s3::{GetObjectError, GetObjectRequest, PutObjectError, PutObjectRequest, S3Client, StreamingBody, S3};
use serde_json::Value;
use snafu::{ResultExt, Snafu};
use std::io::Read;

#[derive(Debug, Snafu, lambda_runtime_errors::LambdaErrorExt)]
pub enum Error {
    #[snafu(display("I/O error: {}", source))]
    InputOutput { source: std::io::Error },

    #[snafu(display("AWS region parse error: {}", source))]
    AwsRegion {
        source: rusoto_signature::region::ParseRegionError,
    },

    #[snafu(display("S3 get error: {}", source))]
    S3Get { source: RusotoError<GetObjectError> },

    #[snafu(display("S3 put error: {}", source))]
    S3Put { source: RusotoError<PutObjectError> },

    #[snafu(display("Unable to JSON encode: {}", source))]
    JsonEnc { source: serde_json::Error },

    #[snafu(display("Unable to do callback request: {}", source))]
    CbReq { source: reqwest::Error },

    #[snafu(display("Unable to process: {}", source))]
    Image { source: imgroll::Error },
}

impl From<Error> for HandlerError {
    fn from(e: Error) -> Self {
        HandlerError::new(e)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(log::Level::Info)?;
    lambda!(handle_event);
    Ok(())
}

fn handle_event(event: Value, _ctx: lambda::Context) -> Result<(), HandlerError> {
    let s3_event: S3Event = serde_json::from_value(event)?;

    for record in s3_event.records {
        let region: Region = record.aws_region.ok_or("region")?.parse().context(AwsRegion {})?;
        let clnt = S3Client::new(region.clone());
        let bucket = record.s3.bucket.name.ok_or("name")?;
        let key = record.s3.object.key.ok_or("key")?;
        let obj = clnt
            .get_object(GetObjectRequest {
                bucket: bucket.clone(),
                key: key.clone(),
                ..Default::default()
            })
            .sync()
            .context(S3Get {})?;
        let meta = obj.metadata.ok_or("metadata")?;
        let cb_url = meta.get("imgroll-cb").ok_or("callback")?;
        let mut buf = Vec::new();
        obj.body
            .ok_or("body")?
            .into_blocking_read()
            .read_to_end(&mut buf)
            .context(InputOutput {})?;
        let (mut photo, files) = imgroll::process_photo(&buf, &key).context(Image {})?;
        for src in &mut photo.source {
            for mut srcset in &mut src.srcset {
                println!("{}", srcset.src);
                srcset.src = format!("https://{}.s3.dualstack.{}.amazonaws.com/{}", &bucket, region.name(), srcset.src);
                println!("{}", srcset.src);
            }
        }
        let json = serde_json::to_string(&photo).context(JsonEnc {})?;
        for (path, bytes) in files {
            clnt.put_object(PutObjectRequest {
                bucket: bucket.clone(),
                key: path,
                body: Some(StreamingBody::from(bytes)),
                ..Default::default()
            })
            .sync()
            .context(S3Put {})?;
        }
        let hclnt = reqwest::Client::new();
        hclnt
            .post(cb_url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(json)
            .send()
            .context(CbReq {})?;
    }

    Ok(())
}
