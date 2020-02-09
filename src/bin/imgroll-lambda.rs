use aws_lambda_events::event::s3::S3Event;
use log::info;
use rusoto_core::{Region, RusotoError};
use rusoto_s3::{GetObjectError, GetObjectRequest, PutObjectError, PutObjectRequest, S3Client, StreamingBody, S3};
use serde_json::Value;
use snafu::{ResultExt, Snafu};
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::Read;
use tokio;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("I/O error: {}", source))]
    InputOutput { source: std::io::Error },

    #[snafu(display("Logging init error: {}", source))]
    SetLogger { source: log::SetLoggerError },

    #[snafu(display("Number conversion error: {}", source))]
    FromInt { source: std::num::TryFromIntError },

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

    #[snafu(display("Some error: {}", info))]
    WTF { info: String },
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error::WTF { info: e.to_owned() }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let func = lambda::handler_fn(func);
    lambda::run(func).await?;
    Ok(())
}

async fn func(event: Value) -> Result<Value, Error> {
    simple_logger::init_with_level(log::Level::Info).context(SetLogger {})?;

    let s3_event: S3Event = serde_json::from_value(event.clone()).context(JsonEnc {})?;

    for record in s3_event.records {
        let region: Region = record.aws_region.ok_or("region")?.parse().context(AwsRegion {})?;
        let clnt = S3Client::new(region.clone());
        let bucket = record.s3.bucket.name.ok_or("name")?;
        let key = record.s3.object.key.ok_or("key")?;
        info!(
            "Processing object key '{}' in bucket '{}' region '{}'",
            &key,
            &bucket,
            region.name()
        );
        let obj = clnt
            .get_object(GetObjectRequest {
                bucket: bucket.clone(),
                key: key.clone(),
                ..Default::default()
            })
            .await
            .context(S3Get {})?;
        let meta = obj.metadata.ok_or("metadata")?;
        let cb_url = meta.get("imgroll-cb").ok_or("callback")?;
        info!("Found callback URL '{}' in metadata", &cb_url);
        let mut buf = Vec::new();
        obj.body
            .ok_or("body")?
            .into_blocking_read()
            .read_to_end(&mut buf)
            .context(InputOutput {})?;
        let (mut photo, files) = imgroll::process_photo(&buf, &key).context(Image {})?;
        for src in &mut photo.source {
            for mut srcset in &mut src.srcset {
                srcset.src = if let Ok(host) = std::env::var("BUCKET_PUBLIC_HOST") {
                    format!("{}/{}", host, srcset.src)
                } else {
                    format!(
                        "https://{}.s3.dualstack.{}.amazonaws.com/{}",
                        &bucket,
                        region.name(),
                        srcset.src
                    )
                };
            }
        }
        info!("Processed photo, metadata: {:?}", &photo);
        let json = serde_json::to_string(&photo).context(JsonEnc {})?;
        for imgroll::OutFile { name, bytes, mimetype } in files {
            info!("Uploading file '{}'", &name);
            let mut file_meta = HashMap::new();
            file_meta.insert("imgroll-original".to_owned(), key.clone());
            clnt.put_object(PutObjectRequest {
                bucket: bucket.clone(),
                key: name,
                acl: Some("public-read".to_owned()),
                metadata: Some(file_meta),
                content_length: Some(bytes.len().try_into().context(FromInt {})?),
                content_type: Some(mimetype),
                content_disposition: Some("inline".to_owned()),
                body: Some(StreamingBody::from(bytes)),
                ..Default::default()
            })
            .await
            .context(S3Put {})?;
        }
        info!("Sending callback request");
        let hclnt = reqwest::Client::new();
        let resp = hclnt
            .post(cb_url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(json)
            .send()
            .await
            .context(CbReq {})?;
        info!("Callback response: {:?}", &resp);
    }

    Ok(event)
}
