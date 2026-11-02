use crate::{ConfigCommand, FolderCommand, LogsCommand, presentation as display};
use anyhow::{Context, Result, bail, ensure};
use pawhide_core::{config,folders,platform::{self, Window, EXCLUDED}};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::{self, Write}, fs, process::Command, thread, time::Duration};

fn entry(w: &Window) -> Value {
    json!({ "application":w.exe, "pid":w.pid, "hwnd":format!("0x{:X}",w.hwnd),
        "title":w.title, "affinity":w.affinity,
        "capture_state":platform::state(w.affinity) })
}
fn matches_name(w: &Window, name: &str) -> bool { w.exe.eq_ignore_ascii_case(name.trim()) || w.exe.eq_ignore_ascii_case(&format!("{}.exe", name.trim())) }
fn target_windows(target: Option<&str>, title: Option<&str>, pick: bool) -> Result<Vec<Window>> {
    ensure!(pick || target.is_some(), "specify an application (PicView.exe), 0xHANDLE or --pick");
    ensure!(title.is_none() || target.is_some(), "--title requires an application or window target");
    let mut windows = match target { Some(x) => platform::resolve(x)?, None => platform::windows()?.into_iter().filter(|w| !w.tool_window).collect() };
    if let Some(substring)=title {
        ensure!(!substring.is_empty(), "--title may not be empty");
        windows.retain(|w|w.title.to_lowercase().contains(&substring.to_lowercase()));
        ensure!(windows.len() <= 1 || pick, "{} windows match that title; refine --title or use --pick",windows.len());
    }
    ensure!(!windows.is_empty(), "no matching visible windows found");
    if pick {
        windows.sort_by(|a,b|a.exe.cmp(&b.exe).then(a.title.cmp(&b.title)));
        println!("{}",display::accent("Select a window"));
        for (i,w) in windows.iter().enumerate() {
            println!("  {:>2}. {:<25} {:<9} {}",i+1,display::brief(&w.exe,25),platform::state(w.affinity),display::brief(&w.title,75));
        }
        print!("Window number (or q to cancel): "); io::stdout().flush()?;
        let mut input=String::new();io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("q") { bail!("selection cancelled") }
        let number:usize=input.trim().parse().context("enter a valid window number")?;
        ensure!((1..=windows.len()).contains(&number),"selection out of range");
        Ok(vec![windows.swap_remove(number-1)])
    } else { Ok(windows) }
}

pub fn list(target:Option<&str>,show_windows:bool,all:bool,json_output:bool)->Result<()> {
    let mut windows=platform::windows()?;
    windows.retain(|w| (all||!w.tool_window) && target.is_none_or(|name| matches_name(w,name)));
    windows.sort_by(|a,b|a.exe.to_lowercase().cmp(&b.exe.to_lowercase()).then(a.title.cmp(&b.title)));
    if json_output { println!("{}",serde_json::to_string_pretty(&windows.iter().map(entry).collect::<Vec<_>>())?);return Ok(()) }
    if windows.is_empty(){println!("No matching windows.");return Ok(())}
    if show_windows || target.is_some() {
        println!("{}", display::accent("Application                    Capture       Window title"));
        for w in &windows {
            let state=platform::state(w.affinity);
            let padded = format!("{state:<13}");
            let state=match w.affinity { Some(EXCLUDED)=>display::good(&padded), None=>display::warn(&padded), _=>padded };
            println!("{:<30} {} {}",display::brief(&w.exe,30),state,display::brief(&w.title,100));
        }
        println!("\n{}",display::dim(&format!("{} windows · use `inspect <exe>` for handles and diagnostics",windows.len())));
    } else {
        let mut groups:BTreeMap<String,(usize,usize,usize)>=BTreeMap::new();
        for w in windows.iter() {let e=groups.entry(w.exe.clone()).or_default();e.0+=1;if w.affinity==Some(EXCLUDED){e.1+=1}if w.affinity.is_none(){e.2+=1}}
        println!("{}",display::accent("Application                     Windows    Protected"));
        for (name,(count,protected,unknown)) in &groups {
            let protected_label=format!("{protected}/{count}{}",if *unknown>0{" ?"}else{""});
            println!("{:<31} {:>6}    {}",display::brief(name,31),count,protected_label);
        }
        println!("\n{}",display::dim(&format!("{} applications · {} windows · `list <exe>` for individual windows",groups.len(),windows.len())));
    }
    Ok(())
}

pub fn change(target: Option<&str>,title: Option<&str>,pick:bool,protect:bool,allow_shell:bool,verbose:bool)->Result<()> {
    let chosen=target_windows(target,title,pick)?;
    if chosen.len()>1 && chosen.iter().any(|w|w.pid==platform::shell_pid()) && !allow_shell {
        bail!("refusing to protect the Windows shell as a group; choose one window with --pick and --allow-shell for experimental use")
    }
    let total=chosen.len();let mut success=0;
    for w in &chosen {
        let outcome=platform::apply_window(w,protect,allow_shell);
        let action=if protect{"Excluded"}else{"Visible"};
        match outcome {
            Ok(true) => { success+=1;
                println!("{} {} — {}{}",display::good("✓"),w.exe,action,if verbose{format!(" · 0x{:X} · {}",w.hwnd,w.title)}else{String::new()});
            },
            Ok(false) => eprintln!("{} {} — Windows affinity could not be verified",display::warn("?"),w.exe),
            Err(err) => eprintln!("{} {} — {err:#}",display::bad("✗"),w.exe),
        }
    }
    println!("{}",display::dim(&format!("{success}/{total} windows verified")));
    ensure!(success==total,"{} window(s) failed or could not be verified",total-success);
    Ok(())
}

pub fn status(target:Option<&str>, verbose:bool,json_output:bool)->Result<()> {
    let found=if let Some(target)=target {let selected=platform::resolve(target)?; ensure!(!selected.is_empty(),"no visible windows match '{target}'");selected}else {platform::windows()?.into_iter().filter(|w|!w.tool_window).collect()};
    if json_output {
        println!("{}",serde_json::to_string_pretty(&json!({"excluded":found.iter().filter(|w|w.affinity==Some(EXCLUDED)).count(),"visible":found.iter().filter(|w|w.affinity==Some(0)).count(),"unknown":found.iter().filter(|w|!matches!(w.affinity,Some(0)|Some(EXCLUDED))).count(),"windows":found.iter().map(entry).collect::<Vec<_>>() }))?);
        return Ok(())
    }
    if target.is_some() {
        for w in &found {
            let mark=if w.affinity==Some(EXCLUDED) {display::good("● Protected")}else if w.affinity==Some(0){"○ Visible".into()}else{display::warn("? Unverified")};
            println!("{} — {}",w.exe,mark);
            println!("  {}",display::brief(&w.title,100));
            if verbose{println!("  HWND 0x{:X} · PID {} · affinity {:?}",w.hwnd,w.pid,w.affinity)}
        }
    }
    let protected=found.iter().filter(|w|w.affinity==Some(EXCLUDED)).count();
    let visible=found.iter().filter(|w|w.affinity==Some(0)).count();
    println!("{}",display::dim(&format!("Protected: {protected} · Visible: {visible} · Unknown: {}",found.len()-protected-visible)));
    Ok(())
}

pub fn inspect(target:&str,json_output:bool)->Result<()> {
    let windows=platform::resolve(target)?;
    ensure!(!windows.is_empty(),"no visible windows match '{target}'");
    if json_output { println!("{}",serde_json::to_string_pretty(&windows.iter().map(entry).collect::<Vec<_>>())?);return Ok(()) }
    for w in windows {
        println!("{}",display::accent(&w.exe));
        println!("  Title:       {}",w.title);
        println!("  HWND:        0x{:X}",w.hwnd);
        println!("  PID:         {}",w.pid);
        println!("  Capture:     {}",platform::state(w.affinity));
        println!("  Tool window: {}",w.tool_window);
    }
    Ok(())
}

pub fn folders(command: FolderCommand) -> Result<()> {
    match command {
        FolderCommand::Add { path } => {
            let canonical = folders::validated_folder(&path)?;
            let mut cfg = config::load()?;
            if !cfg.folders.contains(&canonical) { cfg.folders.push(canonical.clone()); config::save(&cfg)?; }
            println!("Protected folder registered: {}", canonical.display());
            println!("Explorer folder windows will be monitored. For Wolfmark documents, exact file-open notifications remain required.");
            crate::auto::start()?;
        }
        FolderCommand::Remove { path } => {
            // Allow removing missing/renamed folders and old \?\ saved paths.
            let canonical = folders::display_path(&path);
            let mut cfg = config::load()?;
            cfg.folders.retain(|folder| folder != &canonical &&
                folders::validated_folder(folder).ok().as_ref() != Some(&canonical));
            config::save(&cfg)?;
            println!("Folder rule removed: {}", canonical.display());
        }
        FolderCommand::List => {
            let cfg = config::load()?;
            if cfg.folders.is_empty() { println!("No protected folders registered."); }
            else { for folder in &cfg.folders { println!("{}", folder.display()); } }
        }
        FolderCommand::Match { path } => {
            let cfg = config::load()?;
            match folders::match_document(&cfg.folders, &path)? {
                Some(folder) => println!("Protected by {}", folder.display()),
                None => println!("Not protected by a folder rule"),
            }
        }
        FolderCommand::Notify { hwnd, path } => {
            let handle = pawhide_core::parse_hwnd(&hwnd)?.context("use 0x... HWND")?;
            let cfg = config::load()?;
            if let Some(folder) = folders::match_document(&cfg.folders, &path)? {
                let window = platform::resolve(&hwnd)?.into_iter().find(|w| w.hwnd == handle)
                    .context("window handle is not a visible top-level window")?;
                ensure!(platform::can_protect_window(&window), "this application window cannot be protected");
                if window.affinity != Some(EXCLUDED) { crate::auto::protect_bounded(&window)?; }
                println!("Protected {} because document is inside {}", window.exe, folder.display());
            } else {
                println!("No matching protected folder; left capture state unchanged");
            }
        }
    }
    Ok(())
}

pub fn config(command:ConfigCommand)->Result<()> {
    let path=config::config_path()?;
    match command {
        ConfigCommand::Path => println!("{}",path.display()),
        ConfigCommand::Show=>println!("{}",serde_json::to_string_pretty(&config::load()?)?),
        ConfigCommand::Validate=>{config::load()?;println!("{} Configuration valid: {}",display::good("✓"),path.display())},
        ConfigCommand::Edit=>{
            if !path.exists(){config::save(&config::Config::default())?}
            Command::new("notepad.exe").arg(&path).spawn().context("cannot open Notepad")?;
            println!("Editing {}",path.display());
        },
    }
    Ok(())
}

pub fn logs(command:LogsCommand)->Result<()> {
    let path=config::config_path()?.with_file_name("auto.log");
    match command {
        LogsCommand::Path=>println!("{}",path.display()),
        LogsCommand::Show {lines}=>{
            if !path.exists(){println!("No log file yet: {}",path.display());return Ok(())}
            let content=fs::read_to_string(&path)?;
            for line in content.lines().rev().take(lines.min(5000)).collect::<Vec<_>>().into_iter().rev() {println!("{line}")}
        },
        LogsCommand::Follow=>{
            println!("Following {} (Ctrl+C to stop)",path.display());
            let mut offset=fs::metadata(&path).map(|m|m.len()).unwrap_or(0);
            loop {
                if let Ok(content)=fs::read(&path) {
                    if (content.len() as u64)<offset {offset=0}
                    if (content.len() as u64)>offset {
                        print!("{}",String::from_utf8_lossy(&content[offset as usize..]));
                        io::stdout().flush()?;
                        offset=content.len() as u64;
                    }
                }
                thread::sleep(Duration::from_millis(400));
            }
        }
    }
    Ok(())
}
