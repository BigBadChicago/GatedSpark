# Putting GatedSpark on Amazon S3

What this is: the puzzles are plain signed files, so a web host that does nothing but
hand over files is enough. An S3 bucket is exactly that. **No program runs on the
server.** Every check that decides anything — the signature on each puzzle, the
signature on the schedule, the withdrawal list — happens in the browser that opens it.

Two parts: the bucket setup, which is done once by hand, and the upload, which is one
command every time.

## Once: the bucket

Three settings, in the AWS console, on the bucket.

**1. Turn on static website hosting.** Properties → Static website hosting → Enable,
with `index.html` as the index document. Without it the bucket has no front page and
the address below gives a 404.

**2. Let the public read it.** Two steps, and both are needed:

- Permissions → Block public access → untick "Block all public access".
- Permissions → Bucket policy → allow anyone to read the objects:

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "PublicRead",
    "Effect": "Allow",
    "Principal": "*",
    "Action": "s3:GetObject",
    "Resource": "arn:aws:s3:::YOUR-BUCKET-NAME/*"
  }]
}
```

`s3:GetObject` and nothing else. Nobody but you should be able to write, and nobody
needs to list the bucket: the page asks for files by name, and a puzzle's name is its
own fingerprint.

**The same three settings, as commands**, if you would rather not click through the
console. Replace `YOUR-BUCKET-NAME`, and run them from a folder you do not mind writing
a small file into:

```powershell
aws s3 website s3://YOUR-BUCKET-NAME --index-document index.html

aws s3api put-public-access-block --bucket YOUR-BUCKET-NAME `
    --public-access-block-configuration "BlockPublicAcls=false,IgnorePublicAcls=false,BlockPublicPolicy=false,RestrictPublicBuckets=false"

'{"Version":"2012-10-17","Statement":[{"Sid":"PublicRead","Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::YOUR-BUCKET-NAME/*"}]}' | Set-Content -Encoding ascii policy.json
aws s3api put-bucket-policy --bucket YOUR-BUCKET-NAME --policy file://policy.json
```

The middle command is the one that sounds alarming and is not: it is what makes a public
website public. The bucket holds signed puzzles and a page, and the policy that follows
grants reading and nothing else.

**3. Nothing else.** No CORS rules, because the page and the puzzles are on the same
host. No CloudFront, unless you later want a custom address or a certificate.

## Every time: the upload

```bash
# Make the folder.
cargo run -p gs-cli -- publish \
    --store ./store --key ~/gatedspark-dev.key \
    --out ./site --page crates/gs-web/www

# Put it up.
scripts/deploy-s3.sh ./site YOUR-BUCKET-NAME us-east-2
```

On Windows, where there is no bash, the second command has a twin that does the same
thing:

```powershell
scripts\deploy-s3.ps1 .\site YOUR-BUCKET-NAME us-east-2
```

Then the page is at `http://YOUR-BUCKET-NAME.s3-website.us-east-2.amazonaws.com/`.

**The upload needs no build.** The two halves are separate: making the folder needs the
Rust toolchain, putting it up needs only the AWS command line tool. A folder made on one
machine uploads fine from another, which is the way out when a machine cannot build.

## What the upload script does that a plain copy would not

**It refuses to upload a folder containing answer files.** `gatedspark publish` never
copies them, and this is the second lock on the same door, because the script is pointed
at a folder somebody chose and nothing else stops that folder being a store. Publishing
the answers is the one mistake that cannot be taken back: once they are on a public host,
they have been seen.

**It marks the WebAssembly file as `application/wasm`.** A browser refuses to start a
WebAssembly module that arrives as anything else, and the failure reads as a broken page
rather than a wrong file type. S3 guesses a type from the file extension and guesses this
one wrong.

**It caches the three kinds of file differently**, and the reasons are the whole design:

| | Cached for | Why |
|---|---|---|
| The puzzles | A year, immutable | A puzzle's name **is** its fingerprint. If the bytes changed the name would change, so the file at that name can never be a different file. That is a property of how they are named, not a promise anybody keeps. |
| The page and the browser module | Five minutes | They change with each build and their names carry no fingerprint. |
| The schedule and the withdrawal list | Not at all | These are the files that say what has **changed**. |

That last row is the one that matters. A cached withdrawal list is a withdrawal that has
not arrived yet, and that is the one delay in this design that actually hurts somebody:
a player carries on with a puzzle that cannot be finished. The signature and the serial
number stop an old list being *forced* on a device. Only a short cache stops it arriving
late.

**It uploads the puzzles before the schedule.** A schedule that named a day whose puzzle
was not up yet would be a day that breaks for everybody until the next upload finished.

## What this costs

Close to nothing. A puzzle package is about 20 KB. A year of two daily puzzles is under
15 MB of storage, which is a fraction of a cent a month, and the requests are in S3's
free tier at any size this project is likely to reach.

## What this does not give you

- **A proper address.** The S3 website address is long and plain `http`. A custom name
  and a certificate mean CloudFront, which is a decision for when somebody other than you
  is using it.
- **Anything about who is playing.** There is no server, so there is nothing counting
  anybody. That is also why the on-demand channel is not here: handing each player an
  unseen puzzle means remembering what they have seen, and remembering needs a program.
