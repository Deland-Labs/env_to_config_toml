use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use glob::glob;
use env_file_reader::read_file;

fn main() {
    let source = "F:\\Github\\ne8u14\\core-canister\\src\\env_configs\\dev.principals.env";
    let out = "C:\\Users\\77162\\Desktop\\out_cargo\\Cargo.toml";
    match merge_env_files(source, out) {
        Ok(_) => println!("merge env files success"),
        Err(e) => println!("merge env files failed: {}", e),
    }
}


fn merge_env_files(source: &str, out: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 使用 glob 匹配符搜索所有符合条件的文件
    let mut env_files = Vec::new();
    for entry in glob(source).expect("Failed to read glob pattern") {
        let path = entry?;
        env_files.push(path);
    }

    // 将所有文件解析为键值对
    let mut env_vars = std::collections::HashMap::new();
    for env_file in env_files {
        let env_file = read_file(env_file)?;
        for (key, value) in env_file {
            //let value = value.replace("\n", "||||");
            let value = value.trim().lines().filter(|s| !s.starts_with("#")).collect::<Vec<_>>().join("||||");
            env_vars.insert(key, value);
        }
    }


    // 将键值对写入到指定的 toml 文件中
    let toml_path = Path::new(out);
    if toml_path.exists() {
        let mut file = OpenOptions::new().read(true).write(true).open(toml_path)?;
        let reader = BufReader::new(&file);
        let mut writer = BufWriter::new(&file);
        let mut found_env_section = false;
        for line in reader.lines() {
            let line = line?;
            if line.trim() == "[env]" {
                found_env_section = true;
                writer.write_all(b"[env]\n")?;
                for (key, value) in &env_vars {
                    writer.write_all(format!("{} = \"{}\"\n", key, value).as_bytes())?;
                }
            } else if found_env_section {
                break;
            } else {
                writer.write_all(line.as_bytes())?;
                writer.write_all(b"\n")?;
            }
        }
    } else {
        let mut file = File::create(toml_path)?;
        let mut writer = BufWriter::new(&file);
        writer.write_all(b"[env]\n")?;
        for (key, value) in &env_vars {
            writer.write_all(format!("{} = \"{}\"\n", key, value).as_bytes())?;
        }
    };
    Ok(())
}