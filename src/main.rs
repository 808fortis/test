use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct Partition {
    index: String,
    name: String,
    file_name: String,
    is_download: String,
    partition_type: String,
    linear_start_addr: String,
    physical_start_addr: String,
    partition_size: u64,
    region: String,
}

struct ValidationResult {
    is_valid: bool,
    message: String,
    size_ok: bool,
}

fn parse_size(size_str: &str) -> u64 {
    let hex = size_str.trim_start_matches("0x");
    u64::from_str_radix(hex, 16).unwrap_or(0)
}

fn parse_scatter_file<P: AsRef<Path>>(path: P) -> io::Result<Vec<Partition>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut partitions = Vec::new();
    let mut current = HashMap::new();
    
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        
        if line.starts_with("- partition_index:") {
            if !current.is_empty() {
                partitions.push(Partition {
                    index: current.get("index").unwrap_or(&"0".to_string()).clone(),
                    name: current.get("name").unwrap_or(&"".to_string()).clone(),
                    file_name: current.get("file_name").unwrap_or(&"".to_string()).clone(),
                    is_download: current.get("is_download").unwrap_or(&"false".to_string()).clone(),
                    partition_type: current.get("type").unwrap_or(&"".to_string()).clone(),
                    linear_start_addr: current.get("linear_start_addr").unwrap_or(&"0x0".to_string()).clone(),
                    physical_start_addr: current.get("physical_start_addr").unwrap_or(&"0x0".to_string()).clone(),
                    partition_size: parse_size(current.get("partition_size").unwrap_or(&"0x0".to_string())),
                    region: current.get("region").unwrap_or(&"".to_string()).clone(),
                });
            }
            current.clear();
            let idx = line.split(':').nth(1).unwrap_or("0").trim().to_string();
            current.insert("index".to_string(), idx);
        } else if line.contains(':') {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_string();
                let value = parts[1].trim().to_string();
                current.insert(key, value);
            }
        }
    }
    
    if !current.is_empty() {
        partitions.push(Partition {
            index: current.get("index").unwrap_or(&"0".to_string()).clone(),
            name: current.get("name").unwrap_or(&"".to_string()).clone(),
            file_name: current.get("file_name").unwrap_or(&"".to_string()).clone(),
            is_download: current.get("is_download").unwrap_or(&"false".to_string()).clone(),
            partition_type: current.get("type").unwrap_or(&"".to_string()).clone(),
            linear_start_addr: current.get("linear_start_addr").unwrap_or(&"0x0".to_string()).clone(),
            physical_start_addr: current.get("physical_start_addr").unwrap_or(&"0x0".to_string()).clone(),
            partition_size: parse_size(current.get("partition_size").unwrap_or(&"0x0".to_string())),
            region: current.get("region").unwrap_or(&"".to_string()).clone(),
        });
    }
    
    Ok(partitions)
}

fn validate_partition(part: &Partition) -> ValidationResult {
    if part.name == "preloader" {
        return ValidationResult {
            is_valid: false,
            message: "preloader excluded".to_string(),
            size_ok: false,
        };
    }
    
    if part.file_name.is_empty() || part.file_name == "NONE" {
        return ValidationResult {
            is_valid: false,
            message: "no file".to_string(),
            size_ok: false,
        };
    }
    
    if part.is_download.to_lowercase() != "true" {
        return ValidationResult {
            is_valid: false,
            message: "is_download false".to_string(),
            size_ok: false,
        };
    }
    
    let path = Path::new(&part.file_name);
    if !path.exists() {
        return ValidationResult {
            is_valid: false,
            message: "file missing".to_string(),
            size_ok: false,
        };
    }
    
    let file_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let size_ok = file_size <= part.partition_size;
    
    let status = if size_ok { "ok" } else { "overflow" };
    let msg = format!("{} ({} / {})", status, file_size, part.partition_size);
    
    ValidationResult {
        is_valid: true,
        message: msg,
        size_ok,
    }
}

fn get_flashable_partitions(partitions: &[Partition]) -> Vec<(usize, &Partition, ValidationResult)> {
    let mut flashable = Vec::new();
    
    for (idx, part) in partitions.iter().enumerate() {
        let result = validate_partition(part);
        if result.is_valid && result.size_ok {
            flashable.push((idx, part, result));
        } else if result.is_valid && !result.size_ok {
            eprintln!("  warn: {} size overflow", part.name);
        }
    }
    
    flashable
}

fn get_mtk_block_device() -> String {
    let block_devices = [
        "/dev/block/by-name",
        "/dev/block/platform/mtk-msdc.0/by-name",
        "/dev/block/platform/bootdevice/by-name",
        "/dev/block/platform/11230000.mmc/by-name",
    ];
    
    for dev in block_devices.iter() {
        if Path::new(dev).exists() {
            return dev.to_string();
        }
    }
    
    "/dev/block/by-name".to_string()
}

fn generate_update_script(flashable: &[(usize, &Partition, ValidationResult)], block_dev: &str) -> String {
    let mut script = String::new();
    
    script.push_str("#!/sbin/sh\n");
    script.push_str("\n");
    script.push_str("ui_print \"Flashable Firmware\"\n");
    script.push_str("\n");
    script.push_str("ui_print \"Checking device...\"\n");
    script.push_str(&format!("BLOCK_DEV={}\n", block_dev));
    script.push_str("if [ ! -d \"$BLOCK_DEV\" ]; then\n");
    script.push_str("    ui_print \"Error: Block device not found\"\n");
    script.push_str("    exit 1\n");
    script.push_str("fi\n");
    script.push_str("\n");
    script.push_str("ui_print \"Verifying partitions...\"\n");
    
    for (_, part, _) in flashable {
        let part_name = &part.name;
        script.push_str(&format!(
            "if [ ! -e \"$BLOCK_DEV/{}\" ]; then\n",
            part_name
        ));
        script.push_str(&format!("    ui_print \"Error: {} not found\"\n", part_name));
        script.push_str("    exit 1\n");
        script.push_str("fi\n");
    }
    script.push_str("\n");
    script.push_str("ui_print \"Writing firmware...\"\n");
    script.push_str("\n");
    
    for (_, part, _) in flashable {
        script.push_str(&format!("ui_print \"  Writing {}\"\n", part.name));
        script.push_str(&format!(
            "dd if=\"/tmp/install/{}\" of=\"$BLOCK_DEV/{}\" bs=4M 2>/dev/null\n",
            part.file_name, part.name
        ));
        script.push_str(&format!(
            "if [ $? -eq 0 ]; then\n    ui_print \"    OK {}\"\nelse\n    ui_print \"    Failed {}\"\n    exit 1\nfi\n\n",
            part.name, part.name
        ));
    }
    
    script.push_str("sync\n");
    script.push_str("\n");
    script.push_str("ui_print \"\"\n");
    script.push_str("ui_print \"Flash completed successfully\"\n");
    script.push_str("ui_print \"\"\n");
    script.push_str("ui_print \"You can now reboot to system\"\n");
    script.push_str("ui_print \"If you experience bootloop,\" \n");
    script.push_str("ui_print \"wipe data and cache from recovery\"\n");
    script.push_str("ui_print \"\"\n");
    script.push_str("ui_print \"Press any key to return to recovery menu\"\n");
    script.push_str("read key\n");
    
    script
}

fn generate_flashable_zip(
    flashable: &[(usize, &Partition, ValidationResult)],
    temp_dir: &Path,
    block_dev: &str,
) -> io::Result<()> {
    for (_, part, _) in flashable {
        let src = Path::new(&part.file_name);
        let dst = temp_dir.join(&part.file_name);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, &dst)?;
    }
    
    let meta_dir = temp_dir.join("META-INF").join("com").join("google").join("android");
    fs::create_dir_all(&meta_dir)?;
    
    let script_path = meta_dir.join("update-script");
    let script_content = generate_update_script(flashable, block_dev);
    fs::write(&script_path, script_content)?;
    
    let binary_path = meta_dir.join("update-binary");
    fs::write(&binary_path, "#!/sbin/sh\nexit 0\n")?;
    
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::metadata(&binary_path)?.permissions();
        let mut new_perms = perms;
        new_perms.set_mode(0o755);
        fs::set_permissions(&binary_path, new_perms)?;
    }
    
    Ok(())
}

fn find_scatter_file() -> Option<PathBuf> {
    for entry in fs::read_dir(".").ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        if let Some(name_str) = name.to_str() {
            if name_str.ends_with("_Android_Scatter.txt") {
                return Some(entry.path());
            }
        }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    let scatter_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        match find_scatter_file() {
            Some(path) => path,
            None => {
                eprintln!("usage: {} <MTxxx_Android_Scatter.txt>", args[0]);
                std::process::exit(1);
            }
        }
    };
    
    if !scatter_path.exists() {
        eprintln!("error: {}: file not found", scatter_path.display());
        std::process::exit(1);
    }
    
    eprintln!("Flashable Firmware Creator");
    eprintln!("");
    eprintln!("input: {}", scatter_path.display());
    
    let partitions = match parse_scatter_file(&scatter_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: parse failed: {}", e);
            std::process::exit(1);
        }
    };
    
    eprintln!("partitions: {}", partitions.len());
    
    let flashable = get_flashable_partitions(&partitions);
    
    if flashable.is_empty() {
        eprintln!("error: no flashable partitions found");
        std::process::exit(1);
    }
    
    eprintln!("flashable: {}", flashable.len());
    for (i, (_, part, result)) in flashable.iter().enumerate() {
        eprintln!("  {}. {} -> {} [{}]", i+1, part.name, part.file_name, result.message);
    }
    
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let zip_name = format!("flashable_firmware_{}", timestamp);
    let temp_dir = PathBuf::from(&zip_name);
    
    let block_dev = get_mtk_block_device();
    eprintln!("");
    eprintln!("block device: {}", block_dev);
    eprintln!("generating: {}/", zip_name);
    
    if let Err(e) = generate_flashable_zip(&flashable, &temp_dir, &block_dev) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
    
    eprintln!("done");
    eprintln!("");
    eprintln!("output: {}", zip_name);
    eprintln!("");
    eprintln!("to use:");
    eprintln!("  1. copy {} to device", zip_name);
    eprintln!("  2. boot to custom recovery");
    eprintln!("  3. install zip");
    eprintln!("  4. after flash completes, reboot manually from recovery menu");
    eprintln!("  5. if bootloop, wipe data and cache");
    
    let list_file = "firmware_list.txt";
    let mut list = File::create(list_file).unwrap();
    writeln!(list, "# Flashable Firmware List").unwrap();
    writeln!(list, "# Source: {}", scatter_path.file_name().unwrap().to_str().unwrap()).unwrap();
    writeln!(list, "").unwrap();
    for (i, (_, part, res)) in flashable.iter().enumerate() {
        writeln!(list, "[{}] {}: {}", i+1, part.name, part.file_name).unwrap();
        writeln!(list, "    status: {}", res.message).unwrap();
    }
    eprintln!("list: {}", list_file);
  }
