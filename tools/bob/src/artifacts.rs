/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path};
use std::time::Duration;
use std::{fs, thread};

use small_http::{Request, Response};
use url::Url;
use zip::ZipArchive;

pub(crate) fn validate_jar(reader: impl Read + io::Seek) -> io::Result<()> {
    let mut archive = ZipArchive::new(reader).map_err(io::Error::other)?;
    for index in 0..archive.len() {
        archive.by_index(index).map_err(io::Error::other)?;
    }
    Ok(())
}

pub(crate) fn valid_jar(path: &str) -> bool {
    File::open(path).and_then(validate_jar).is_ok()
}

pub(crate) fn download_jar(url: &str, destination: &str, lock_file: &str) -> io::Result<()> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_file)?;
    lock.lock()?;
    if valid_jar(destination) {
        return Ok(());
    }
    // Remove corrupt cache entries while holding the lock, including on Windows.
    if Path::new(destination).exists() {
        fs::remove_file(destination)?;
    }
    let temporary = format!("{destination}.part");
    let result = download_bytes(url).and_then(|bytes| {
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, destination)
    });
    if result.is_err() {
        _ = fs::remove_file(&temporary);
    }
    result
}

fn download_bytes(url: &str) -> io::Result<Vec<u8>> {
    download_candidates(&download_urls(url))
}

fn download_candidates(urls: &[String]) -> io::Result<Vec<u8>> {
    let mut last_error = io::Error::other("no download attempted");
    for attempt in 0..3 {
        let mut retry = false;
        for candidate in urls {
            let result = fetch(candidate).and_then(|response| {
                let status = response.status as u16;
                if status != 200 {
                    let kind = if (400..500).contains(&status) && !matches!(status, 408 | 429) {
                        io::ErrorKind::NotFound
                    } else {
                        io::ErrorKind::Other
                    };
                    return Err(io::Error::new(kind, format!("HTTP {status}")));
                }
                validate_jar(Cursor::new(&response.body))?;
                Ok(response.body)
            });
            match result {
                Ok(bytes) => return Ok(bytes),
                Err(error) => {
                    eprintln!("Failed to download {candidate}: {error}");
                    retry |= error.kind() != io::ErrorKind::NotFound;
                    last_error = error;
                }
            }
        }
        if !retry {
            break;
        }
        if attempt < 2 {
            thread::sleep(Duration::from_millis(250 << attempt));
        }
    }
    Err(io::Error::other(format!(
        "download failed for {}: {last_error}",
        urls.join(" or ")
    )))
}

fn download_urls(url: &str) -> Vec<String> {
    for host in [
        "https://repo.maven.apache.org/maven2/",
        "https://repo1.maven.org/maven2/",
    ] {
        if let Some(path) = url.strip_prefix(host) {
            return vec![
                format!("https://repo.maven.apache.org/maven2/{path}"),
                format!("https://repo1.maven.org/maven2/{path}"),
            ];
        }
    }
    vec![url.to_owned()]
}

fn fetch(url: &str) -> io::Result<Response> {
    let url: Url = url.parse().map_err(io::Error::other)?;
    if url.userinfo().is_some() {
        return Err(io::Error::other("expected a URL without credentials"));
    }
    Request::new()
        .url(url)
        .header("User-Agent", concat!("bob/", env!("CARGO_PKG_VERSION")))
        .fetch()
        .map_err(io::Error::other)
}

pub(crate) fn extract_jar(source: &str, destination: &str) -> io::Result<()> {
    let mut archive = ZipArchive::new(File::open(source)?).map_err(io::Error::other)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        let name = entry.name();
        if name.starts_with("META-INF/") || !name.ends_with(".class") {
            continue;
        }
        if name.contains(['\\', ':'])
            || Path::new(name)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(io::Error::other(format!("unsafe JAR entry: {name}")));
        }
        let path = Path::new(destination).join(name);
        fs::create_dir_all(path.parent().expect("class file has a parent"))?;
        io::copy(&mut entry, &mut File::create(path)?)?;
    }
    Ok(())
}

pub(crate) fn add_dex(source: &str, dex: &str, destination: &str) -> io::Result<()> {
    fs::copy(source, destination)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)?;
    zip::append_file(file, "classes.dex", &fs::read(dex)?).map_err(io::Error::other)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("bob-artifacts-{name}-{}", std::process::id()));
        _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn jar() -> Vec<u8> {
        let mut bytes = b"PK\x05\x06".to_vec();
        bytes.extend_from_slice(&[0; 18]);
        let mut reader = Cursor::new(bytes);
        zip::append_file(&mut reader, "example/Test.class", b"bytecode").unwrap();
        reader.into_inner()
    }

    fn response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn server(responses: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/artifact.jar", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "timed out waiting for request");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("accept failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    assert!(!line.is_empty());
                }
                requests.push(request);
                stream.write_all(&response).unwrap();
            }
            requests
        });
        (url, handle)
    }

    #[test]
    fn retries_truncated_responses_and_invalid_jars_before_caching() {
        let directory = scratch("retry");
        let destination = directory.join("artifact.jar");
        fs::write(&destination, b"corrupt cache").unwrap();
        let bytes = jar();
        let (url, server) = server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\npartial".to_vec(),
            response("200 OK", b"not a jar"),
            response("200 OK", &bytes),
        ]);
        download_jar(
            &url,
            destination.to_str().unwrap(),
            directory.join("lock").to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(server.join().unwrap().len(), 3);
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        assert!(!directory.join("artifact.jar.part").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn follows_redirects_and_retries_server_errors() {
        let (url, server) = server(vec![
            response("503 Service Unavailable", b"busy"),
            b"HTTP/1.1 302 Found\r\nLocation: /actual.jar\r\nContent-Length: 0\r\n\r\n".to_vec(),
            response("200 OK", &jar()),
        ]);
        assert_eq!(download_bytes(&url).unwrap(), jar());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].starts_with("GET /actual.jar "));
    }

    #[test]
    fn missing_artifacts_are_not_retried_or_cached() {
        let directory = scratch("missing");
        let destination = directory.join("artifact.jar");
        let (url, server) = server(vec![response("404 Not Found", b"missing")]);
        assert!(
            download_jar(
                &url,
                destination.to_str().unwrap(),
                directory.join("lock").to_str().unwrap()
            )
            .is_err()
        );
        assert_eq!(server.join().unwrap().len(), 1);
        assert!(!destination.exists());
        assert!(!directory.join("artifact.jar.part").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn concurrent_downloads_reuse_the_locked_cache() {
        let directory = scratch("concurrent");
        let (url, server) = server(vec![response("200 OK", &jar())]);
        thread::scope(|scope| {
            let handles = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        download_jar(
                            &url,
                            directory.join("artifact.jar").to_str().unwrap(),
                            directory.join("lock").to_str().unwrap(),
                        )
                        .unwrap();
                    })
                })
                .collect::<Vec<_>>();
            for handle in handles {
                handle.join().unwrap();
            }
        });
        assert_eq!(server.join().unwrap().len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn extracts_only_classes_outside_meta_inf_and_preserves_apk_entries() {
        let directory = scratch("archives");
        let mut bytes = Cursor::new(jar());
        zip::append_file(&mut bytes, "META-INF/versions/9/Test.class", b"excluded").unwrap();
        zip::append_file(&mut bytes, "resource.txt", b"resource").unwrap();
        let source = directory.join("source.jar");
        fs::write(&source, bytes.into_inner()).unwrap();
        let classes = directory.join("classes");
        extract_jar(source.to_str().unwrap(), classes.to_str().unwrap()).unwrap();
        assert_eq!(
            fs::read(classes.join("example/Test.class")).unwrap(),
            b"bytecode"
        );
        assert!(!classes.join("META-INF").exists());
        assert!(!classes.join("resource.txt").exists());

        let dex = directory.join("classes.dex");
        fs::write(&dex, b"dex").unwrap();
        let apk = directory.join("app.apk");
        add_dex(
            source.to_str().unwrap(),
            dex.to_str().unwrap(),
            apk.to_str().unwrap(),
        )
        .unwrap();
        let mut archive = ZipArchive::new(File::open(apk).unwrap()).unwrap();
        assert_eq!(archive.len(), 4);
        let mut data = Vec::new();
        archive.by_index(3).unwrap().read_to_end(&mut data).unwrap();
        assert_eq!(data, b"dex");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn central_urls_have_a_primary_and_fallback() {
        let urls = download_urls("https://repo1.maven.org/maven2/example/artifact.jar");
        assert_eq!(
            urls,
            [
                "https://repo.maven.apache.org/maven2/example/artifact.jar",
                "https://repo1.maven.org/maven2/example/artifact.jar",
            ]
        );
        assert_eq!(
            download_urls("https://example.org/custom.jar"),
            ["https://example.org/custom.jar"]
        );
    }

    #[test]
    fn tries_the_fallback_host_after_a_failed_primary() {
        let (primary, primary_server) = server(vec![response("429 Too Many Requests", b"busy")]);
        let (fallback, fallback_server) = server(vec![response("200 OK", &jar())]);
        assert_eq!(download_candidates(&[primary, fallback]).unwrap(), jar());
        assert_eq!(primary_server.join().unwrap().len(), 1);
        assert_eq!(fallback_server.join().unwrap().len(), 1);
    }

    #[test]
    fn rejects_bad_entry_crcs_and_absolute_extraction_paths() {
        let directory = scratch("unsafe");
        let bytes = jar();
        let mut corrupt = bytes.clone();
        let offset = corrupt
            .windows(8)
            .position(|window| window == b"bytecode")
            .unwrap();
        corrupt[offset] ^= 1;
        assert!(validate_jar(Cursor::new(corrupt)).is_err());

        let mut unsafe_jar = bytes;
        let original = b"example/Test.class";
        let replacement = b"/escape/Test.class";
        for index in 0..=unsafe_jar.len() - original.len() {
            if unsafe_jar[index..index + original.len()] == *original {
                unsafe_jar[index..index + original.len()].copy_from_slice(replacement);
            }
        }
        let source = directory.join("unsafe.jar");
        fs::write(&source, unsafe_jar).unwrap();
        assert!(
            extract_jar(
                source.to_str().unwrap(),
                directory.join("classes").to_str().unwrap()
            )
            .is_err()
        );
        assert!(!directory.join("classes").exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
