# imgroll

A smart image optimization library / local executable / AWS Lambda function.
Primarily designed for [sweetroll2].

[sweetroll2]: https://github.com/myfreeweb/sweetroll2

- Extracts some useful metadata using exiv2
- Applies rotation specified in metadata
- Generates [tiny WebP data URI placeholders/previews](https://jmperezperez.com/webp-placeholder-images/)
- Extracts a color palette using [color-thief](https://github.com/RazrFalcon/color-thief-rs)
- Produces up to three sizes for each output format
- Processes output formats in parallel 
- Outputs a JSON object describing the resulting images and the extracted metadata

The output formats depend on the input format.

- For PNGs:
	- quantizes colors with [exoquant](https://github.com/exoticorn/exoquant-rs)
	- outputs PNGs compressed with [the Rust port](https://github.com/carols10cents/zopfli) of Zopfli
- For JPEGs:
	- outputs progressive JPEGs compressed with [MozJPEG](https://github.com/mozilla/mozjpeg)
	- outputs WebPs compressed with libwebp

The Lambda function responds to S3 uploads that contain `imgroll-cb` in metadata.
That value is used as a "processing done" callback, sending a JSON body
with the resulting object.
The `BUCKET_PUBLIC_HOST` environment variable can be used to specify a host
for use in output URLs instead of the default S3 host (for use with CloudFront/CNAMEs).

## Schema/Examples

```json
{
  "aperture": 10,
  "focal_length": 27,
  "geo": null,
  "height": 2916,
  "iso": 100,
  "palette": [
    { "b": 106, "g": 89, "r": 58 },
    { "b": 198, "g": 201, "r": 201 },
    { "b": 140, "g": 143, "r": 146 },
    { "b": 25, "g": 23, "r": 2 },
    { "b": 41, "g": 47, "r": 52 },
    { "b": 181, "g": 149, "r": 101 },
    { "b": 191, "g": 183, "r": 159 },
    { "b": 153, "g": 141, "r": 113 },
    { "b": 128, "g": 140, "r": 172 }
  ],
  "shutter_speed": [ 1, 320 ],
  "source": [
    {
      "original": false,
      "srcset": [
        {
          "src": "https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.3000.jpg",
          "width": 3000
        },
        {
          "src": "https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.2000.jpg",
          "width": 2000
        },
        {
          "src": "https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.1000.jpg",
          "width": 1000
        }
      ],
      "type": "image/jpeg"
    },
    {
      "original": false,
      "srcset": [
        {
          "src": "https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.3000.webp",
          "width": 3000
        },
        {
          "src": "https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.2000.webp",
          "width": 2000
        },
        {
          "src": "https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.1000.webp",
          "width": 1000
        }
      ],
      "type": "image/webp"
    },
    {
      "original": true,
      "srcset": [
        {
          "src": "https://dl.unrelenting.technology/IMG-7081.jpg",
          "width": 5184
        }
      ],
      "type": "image/jpeg"
    }
  ],
  "tiny_preview": "data:image/webp;base64,UklGRnAAAABXRUJQVlA4IGQAAAAwBACdASowABoAP93k6Gy/urEptVv8A/A7iWpn5FtTI0FdNumdDYJBregA/QjOCu+Vax2w/NNsn1WlEoWM/p71MMMgguqBQEtfbHi8eOBhwhKVvNAzA0Rvwyv7z3kaGgxQoYAA",
  "width": 5184
}
```

And here's the kind of HTML sweetroll2 can generate from that:

```html
<figure class="entry-photo">
  <responsive-container style="padding-bottom: 56.25%; background: rgb(58, 89, 106) url(&quot;data:image/webp;base64,UklGRnAAAABXRUJQVlA4IGQAAAAwBACdASowABoAP93k6Gy/urEptVv8A/A7iWpn5FtTI0FdNumdDYJBregA/QjOCu+Vax2w/NNsn1WlEoWM/p71MMMgguqBQEtfbHi8eOBhwhKVvNAzA0Rvwyv7z3kaGgxQoYAA&quot;) repeat scroll 0% 0%;">
    <picture>
      <source srcset="https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.3000.webp 3000w,
                      https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.2000.webp 2000w,
                      https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.1000.webp 1000w" type="image/webp">
      <img alt="" class="u-photo"
           src="https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.3000.jpg"
           srcset="https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.3000.jpg 3000w,
                   https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.2000.jpg 2000w,
                   https://dl.unrelenting.technology/4a0c45a0ed40_img-7081.1000.jpg 1000w">
    </picture>
  </responsive-container>
  <figcaption class="entry-photo-meta">
    <svg aria-hidden="false" class="icon" role="image" title="Photo parameters"><use xlink:href="/__as__/icons.svg?vsn=tMCAZl1CLU8yDTq2PdhyDgna#eye"><title>Photo parameters</title></use></svg>
    <span class="camera-focal-length">27 mm</span>
    <span class="camera-iso">ISO 100</span>
    <span class="camera-shutter">1/320</span>
    <span class="camera-aperture">ƒ/10</span>
    <svg aria-hidden="true" class="icon" role="image"><use xlink:href="/__as__/icons.svg?vsn=tMCAZl1CLU8yDTq2PdhyDgna#desktop-download"></use></svg>
    <a class="camera-original" href="https://dl.unrelenting.technology/IMG-7081.jpg">Download original</a>
  </figcaption>
</figure>
```

## License

This is free and unencumbered software released into the public domain.  
For more information, please refer to the `UNLICENSE` file or [unlicense.org](http://unlicense.org).

(Note: different licenses apply to dependencies.)
