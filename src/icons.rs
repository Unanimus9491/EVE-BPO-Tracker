//! Item images from EVE's image server, loaded on background threads and cached on disk.
//! Cache file names match the Python version, so an existing icon cache is reused.

use crate::data::{Entry, USER_AGENT};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Condvar, Mutex};

/// Newest requests first, so what's on screen now loads before cards you scrolled past.
type JobStack = Arc<(Mutex<Vec<Job>>, Condvar)>;

const WORKERS: usize = 6;

enum Slot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

struct Job {
    key: String,
    url: String,
    path: PathBuf,
}

pub struct Icons {
    slots: HashMap<String, Slot>,
    jobs: JobStack,
    results: Receiver<(String, Option<ColorImage>)>,
    dir: PathBuf,
}

pub fn key_and_url(e: &Entry) -> (String, String) {
    if e.tab == "Ships" {
        (format!("r{}", e.product_id), format!("https://images.evetech.net/types/{}/render?size=128", e.product_id))
    } else if e.kind == "reaction" {
        (format!("b{}", e.id), format!("https://images.evetech.net/types/{}/bp?size=64", e.id))
    } else {
        (format!("i{}", e.product_id), format!("https://images.evetech.net/types/{}/icon?size=64", e.product_id))
    }
}

fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

fn fetch(job: &Job) -> Option<ColorImage> {
    if let Ok(bytes) = std::fs::read(&job.path)
        && let Some(img) = decode(&bytes) {
            return Some(img);
        }
    let mut resp = ureq::get(&job.url).header("User-Agent", USER_AGENT).call().ok()?;
    let mut bytes = Vec::new();
    resp.body_mut().as_reader().take(4 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    let img = decode(&bytes)?;
    let _ = std::fs::write(&job.path, &bytes);
    Some(img)
}

impl Icons {
    pub fn new(dir: PathBuf, ctx: egui::Context) -> Self {
        let jobs: JobStack = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let (res_tx, res_rx) = channel();
        for _ in 0..WORKERS {
            let jobs = jobs.clone();
            let tx = res_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let (lock, cvar) = &*jobs;
                        let Ok(mut stack) = lock.lock() else { return };
                        loop {
                            if let Some(j) = stack.pop() {
                                break j;
                            }
                            stack = match cvar.wait(stack) {
                                Ok(s) => s,
                                Err(_) => return,
                            };
                        }
                    };
                    let img = fetch(&job);
                    if tx.send((job.key, img)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self { slots: HashMap::new(), jobs, results: res_rx, dir }
    }

    /// Turn finished downloads into textures. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, img)) = self.results.try_recv() {
            let slot = match img {
                Some(img) => Slot::Ready(ctx.load_texture(&key, img, TextureOptions::LINEAR)),
                None => Slot::Failed, // don't retry this session
            };
            self.slots.insert(key, slot);
        }
    }

    /// Texture for an entry, queuing a load the first time it's asked for.
    pub fn get(&mut self, e: &Entry) -> Option<&TextureHandle> {
        let (key, url) = key_and_url(e);
        if !self.slots.contains_key(&key) {
            let path = self.dir.join(format!("{key}.png"));
            self.slots.insert(key.clone(), Slot::Loading);
            let (lock, cvar) = &*self.jobs;
            if let Ok(mut stack) = lock.lock() {
                stack.push(Job { key: key.clone(), url, path });
                cvar.notify_one();
            }
        }
        match self.slots.get(&key) {
            Some(Slot::Ready(t)) => Some(t),
            _ => None,
        }
    }
}
