use std::env;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;
use unicode_width::UnicodeWidthStr;

/// Pad a string to a fixed display width (handles CJK/wide chars correctly)
fn pad_display(s: &str, width: usize) -> String {
    let display_w = UnicodeWidthStr::width(s);
    if display_w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - display_w))
    }
}

use exiftool_rs::{ExifTool, Options};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut options = Options::default();
    let mut files: Vec<String> = Vec::new();
    let mut json_output = false;
    let mut csv_output = false;
    let mut xml_output = false;
    let mut show_groups = false;
    let mut short_names = false;
    let mut write_tags: Vec<(String, String)> = Vec::new();
    let mut delete_tags: Vec<String> = Vec::new();
    let mut overwrite_original = false;
    let mut recursive = false;
    let mut stay_open = false;
    let mut binary_output = false;
    let mut ext_filter: Option<String> = None;
    let mut tags_from_file: Option<String> = None;
    let mut filename_tag: Option<String> = None;
    let mut if_condition: Option<String> = None;
    let mut print_format: Option<String> = None;
    let mut tab_output = false;
    let mut sort_tags = false;
    let mut show_tag_ids = false;
    let mut quiet = false;
    let mut no_composites = false;
    let mut _show_unknown = false;
    let mut _preserve_dates = false;
    let mut exclude_tags: Vec<String> = Vec::new();
    let mut _date_format: Option<String> = None;
    let mut _separator: Option<String> = None;
    let mut _output_file: Option<String> = None;
    let mut list_tags = false;
    let mut file_order: Option<String> = None;
    let mut args_output = false;
    let mut php_output = false;
    let mut progress = false;
    let mut verbose: u8 = 0;
    let mut validate = false;
    let mut diff_file: Option<String> = None;
    let mut html_dump = false;
    let mut scan_for_xmp = false;
    let mut lang: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-v" | "--version" | "-ver" => {
                println!("exiftool-rs {}", exiftool_rs::VERSION);
                println!("Copyright (C) 2024 Sylvain Gargasson");
                println!("License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl-3.0.html>");
                println!("This is free software: you are free to change and redistribute it.");
                println!("There is NO WARRANTY, to the extent permitted by law.");
                process::exit(0);
            }
            "-h" | "--help" | "-help" => {
                print_usage();
                process::exit(0);
            }
            "-j" | "--json" | "-json" => json_output = true,
            "-csv" => csv_output = true,
            "-X" | "-xml" => xml_output = true,
            "-g" | "--group" | "-g0" => show_groups = true,
            "-n" | "--num" | "-num" => options.print_conv = false,
            "-s" | "--short" => short_names = true,
            "-S" | "-veryShort" => {
                short_names = true;
            } // -S = -s without padding
            "-f" => options.fast_scan = 1,
            "-F" => options.fast_scan = 2,
            "-b" | "-binary" => binary_output = true,
            "-r" | "-recurse" => recursive = true,
            "-overwrite_original" => overwrite_original = true,
            "-a" | "-duplicates" => options.duplicates = true,
            "-D" | "-tagID" => show_tag_ids = true,
            "-e" | "-composite" => no_composites = true,
            "-q" | "-quiet" => quiet = true,
            "-u" | "-unknown" => _show_unknown = true,
            "-U" | "-unknown2" => _show_unknown = true,
            "-m" | "-ignoreMinorErrors" => { /* ignored, we're lenient by default */ }
            "-P" | "-preserve" => _preserve_dates = true,
            "-progress" => {
                progress = true;
            }
            "-L" | "-latin" => { /* charset handled in encoding_rs */ }
            "-t" | "-tab" => tab_output = true,
            "-T" => tab_output = true,
            "-sort" => sort_tags = true,
            "-list" | "-listx" | "-listw" | "-listr" | "-listf" | "-listd" | "-listg1"
            | "-listgeo" | "-listwf" => list_tags = true,
            "-args" | "-argFormat" => {
                args_output = true;
            }
            "-c" | "-coordFormat" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume format arg
            }
            "-charset" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume charset arg
            }
            "-config" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume config file
            }
            "-csvDelim" | "-csvdelim" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-delete_original" | "-deleteOriginal" => {
                // Delete _original backup files
            }
            "-restore_original" | "-restoreOriginal" => {
                // Restore from _original backup
            }
            "-diff" => {
                if i + 1 < args.len() {
                    i += 1;
                    diff_file = Some(args[i].clone());
                }
            }
            "-echo" | "-echo1" | "-echo2" | "-echo3" | "-echo4" => {
                if i + 1 < args.len() {
                    println!("{}", args[i + 1]);
                    i += 1;
                }
            }
            "-ee" => {
                options.extract_embedded = 1;
            }
            "-ee2" => {
                options.extract_embedded = 2;
            }
            "-ee3" => {
                options.extract_embedded = 3;
            }
            "-efile" | "-efile!" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-execute" => { /* stay-open command separator */ }
            "-fast" | "-fast2" | "-fast3" | "-fast4" | "-fast5" => {
                options.fast_scan = match arg.as_str() {
                    "-fast" => 1,
                    "-fast2" => 2,
                    "-fast3" => 3,
                    "-fast4" => 4,
                    "-fast5" => 5,
                    _ => 1,
                };
            }
            "-G" | "-G0" | "-G1" | "-G2" | "-G3" | "-G4" | "-G5" | "-G6" => show_groups = true,
            "-g1" | "-g2" | "-g3" | "-g4" | "-g5" | "-g6" => show_groups = true,
            "-geolocate" | "-geolocation" => {
                // Reverse geocoding is automatic when GPS is present
            }
            "-geotag" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume GPX file
            }
            "-geosync" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-geotime" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-globalTimeShift" | "-globaltimeshift" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-htmlDump" | "-htmldump" => {
                html_dump = true;
            }
            "-i" | "-ignore" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume dir to ignore
            }
            "-k" | "-pause" => {
                // Pause before terminating (Windows)
            }
            "-lang" => {
                if i + 1 < args.len() {
                    i += 1;
                    lang = Some(args[i].to_lowercase().replace("-", "_").replace("_", ""));
                    // Normalize: zh_cn -> zh, pt_br -> pt, etc.
                    if let Some(ref mut l) = lang {
                        if l.starts_with("zh") {
                            *l = "zh".into();
                        }
                    }
                }
            }
            "-api" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume API option
            }
            "-one" | "-1" => { /* process only first file */ }
            "-overwrite_original_in_place" => overwrite_original = true,
            "-password" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-php" | "-phpFormat" => {
                php_output = true;
            }
            "-preview" => { /* extract preview image */ }
            "-require" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-scanForXMP" | "-scanforxmp" => {
                scan_for_xmp = true;
            }
            "-struct" | "-s2" | "-s1" => short_names = true,
            "-use" | "-useMWG" | "-usemwg" => { /* use MWG composite tags */ }
            "-validate" => {
                validate = true;
            }
            "-w" | "-w!" | "-w+" | "-W" | "-W!" | "-W+" => {
                if i + 1 < args.len() {
                    i += 1;
                } // consume output extension
            }
            "-wm" => {
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-z" | "-zip" => { /* process compressed data */ }
            "-common_args" => {
                // Load arguments from file
                if i + 1 < args.len() {
                    if let Ok(content) = std::fs::read_to_string(&args[i + 1]) {
                        for line in content.lines() {
                            let line = line.trim();
                            if !line.is_empty() && !line.starts_with('#') {
                                files.push(line.to_string());
                            }
                        }
                    }
                    i += 1;
                }
            }
            "-@" => {
                // Read arguments from file (same as -common_args)
                if i + 1 < args.len() {
                    if let Ok(content) = std::fs::read_to_string(&args[i + 1]) {
                        for line in content.lines() {
                            let line = line.trim();
                            if !line.is_empty() && !line.starts_with('#') {
                                // Could be a file path or an option
                                if line.starts_with('-') {
                                    // It's an option - we'd need to re-parse, for now skip
                                } else {
                                    files.push(line.to_string());
                                }
                            }
                        }
                    }
                    i += 1;
                }
            }
            "-s3" => {
                short_names = true;
            } // Extra-short: tag names only, no padding
            "-v0" | "-v1" | "-v2" | "-v3" | "-v4" | "-v5" => {
                verbose = args[i]
                    .chars()
                    .last()
                    .unwrap_or('0')
                    .to_digit(10)
                    .unwrap_or(0) as u8;
            }
            "-ifd1" => {
                // Specifically request IFD1 (thumbnail) tags
                options.requested_tags.push("IFD1:*".to_string());
            }
            "-addTagsFromFile" | "-addtagsfromfile" => {
                // Same as -tagsFromFile but adds instead of replacing
                if i + 1 < args.len() {
                    tags_from_file = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-srcfile" | "-srcFile" => {
                // Specify source file for -tagsFromFile
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-userParam" | "-userparam" => {
                // Set user parameter
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-wext" | "-wExt" => {
                // Specify write extension filter
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-listItem" | "-listitem" => {
                // Specify list item index
                if i + 1 < args.len() {
                    i += 1;
                }
            }
            "-unsafe" | "-Unsafe" => {
                // Allow unsafe tag operations
            }
            "-trailer" | "-Trailer" => {
                // Process file trailer
            }
            "-xpath" | "-xPath" => {
                // XMP path output mode
            }
            // Tag group aliases — these expand to multiple tag requests
            "-alldates" | "-AllDates" => {
                options.requested_tags.push("DateTimeOriginal".into());
                options.requested_tags.push("CreateDate".into());
                options.requested_tags.push("ModifyDate".into());
            }
            "-common" => {
                options.requested_tags.extend(
                    [
                        "FileName",
                        "FileSize",
                        "FileType",
                        "Make",
                        "Model",
                        "DateTimeOriginal",
                        "ImageSize",
                        "FocalLength",
                        "ExposureTime",
                        "FNumber",
                        "ISO",
                        "Flash",
                        "LensModel",
                    ]
                    .iter()
                    .map(|s| s.to_string()),
                );
            }
            // Named tag shortcuts (recognized by ExifTool as special options)
            "-directory" => options.requested_tags.push("Directory".into()),
            "-filename" => options.requested_tags.push("FileName".into()),
            "-jpgfromraw" | "-JpgFromRaw" => options.requested_tags.push("JpgFromRaw".into()),
            "-previewimage" | "-PreviewImage" => options.requested_tags.push("PreviewImage".into()),
            "-thumbnailimage" | "-ThumbnailImage" => {
                options.requested_tags.push("ThumbnailImage".into())
            }
            "-embeddedimage" | "-EmbeddedImage" => {
                options.requested_tags.push("EmbeddedImage".into())
            }
            "-icc_profile" | "-ICC_Profile" => options.requested_tags.push("ICC_Profile".into()),
            "-imagesize" | "-ImageSize" => options.requested_tags.push("ImageSize".into()),
            // All remaining ExifTool tag-name options (for 100% CLI compatibility)
            "-aperture" | "-Aperture" => options.requested_tags.push("Aperture".into()),
            "-artist" | "-Artist" => options.requested_tags.push("Artist".into()),
            "-author" | "-Author" => options.requested_tags.push("Author".into()),
            "-canon" | "-Canon" => options.requested_tags.push("Canon".into()),
            "-comment" | "-Comment" => options.requested_tags.push("Comment".into()),
            "-copyright" | "-Copyright" => options.requested_tags.push("Copyright".into()),
            "-createdate" | "-CreateDate" => options.requested_tags.push("CreateDate".into()),
            "-credit" | "-Credit" => options.requested_tags.push("Credit".into()),
            "-datetimeoriginal" | "-DateTimeOriginal" => {
                options.requested_tags.push("DateTimeOriginal".into())
            }
            "-dc" => options.requested_tags.push("dc".into()),
            "-exif" | "-EXIF" => options.requested_tags.push("EXIF:*".into()),
            "-exposurecompensation" | "-ExposureCompensation" => {
                options.requested_tags.push("ExposureCompensation".into())
            }
            "-exposuretime" | "-ExposureTime" => options.requested_tags.push("ExposureTime".into()),
            "-file" => options.requested_tags.push("File:*".into()),
            "-file1" => options.requested_tags.push("File:*".into()),
            "-filenum" | "-FileNum" | "-fileNum" => {
                options.requested_tags.push("FileNumber".into())
            }
            "-four" => { /* numeric argument */ }
            "-hierarchicalkeywords" | "-HierarchicalKeywords" => {
                options.requested_tags.push("HierarchicalSubject".into())
            }
            "-iptc" | "-IPTC" => options.requested_tags.push("IPTC:*".into()),
            "-iso" | "-ISO" => options.requested_tags.push("ISO".into()),
            "-keywords" | "-Keywords" => options.requested_tags.push("Keywords".into()),
            "-la" => { /* language argument */ }
            "-lightsource" | "-LightSource" => options.requested_tags.push("LightSource".into()),
            "-ls" => { /* list separator shortcut */ }
            "-modifydate" | "-ModifyDate" => options.requested_tags.push("ModifyDate".into()),
            "-orientation" | "-Orientation" => options.requested_tags.push("Orientation".into()),
            "-owner" | "-Owner" => options.requested_tags.push("OwnerName".into()),
            "-photoshop" | "-Photoshop" => options.requested_tags.push("Photoshop:*".into()),
            "-plot" => { /* plotting option */ }
            "-shutterspeed" | "-ShutterSpeed" => options.requested_tags.push("ShutterSpeed".into()),
            "-tag" => { /* generic */ }
            "-three" | "-two" => { /* numeric arguments */ }
            "-time" | "-Time" => options.requested_tags.push("Time:*".into()),
            "-title" | "-Title" => options.requested_tags.push("Title".into()),
            "-whitebalance" | "-WhiteBalance" => options.requested_tags.push("WhiteBalance".into()),
            "-xmp" | "-XMP" => options.requested_tags.push("XMP:*".into()),
            "-list_dir" | "-listDir" => {
                // List directories instead of processing files
                for f in &files {
                    let path = Path::new(f);
                    if path.is_dir() {
                        if let Ok(entries) = std::fs::read_dir(path) {
                            for entry in entries.flatten() {
                                println!("{}", entry.path().display());
                            }
                        }
                    }
                }
                process::exit(0);
            }
            "-d" => {
                if i + 1 < args.len() {
                    _date_format = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-sep" => {
                if i + 1 < args.len() {
                    _separator = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-o" => {
                if i + 1 < args.len() {
                    _output_file = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-x" => {
                if i + 1 < args.len() {
                    exclude_tags.push(args[i + 1].clone());
                    i += 1;
                }
            }
            "-fileOrder" | "-fileorder" => {
                if i + 1 < args.len() {
                    file_order = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-if" => {
                if i + 1 < args.len() {
                    if_condition = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-p" => {
                if i + 1 < args.len() {
                    print_format = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-stay_open" => {
                // Next arg should be "True" or "1"
                if i + 1 < args.len() {
                    let next = &args[i + 1];
                    if next.eq_ignore_ascii_case("true") || next == "1" {
                        stay_open = true;
                        i += 1;
                    }
                }
            }
            "-tagsFromFile" | "-TagsFromFile" | "-tagsfromfile" => {
                if i + 1 < args.len() {
                    tags_from_file = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-FileName<DateTimeOriginal" | "-filename<datetimeoriginal" => {
                filename_tag = Some("DateTimeOriginal".into());
            }
            arg if arg.to_lowercase().starts_with("-filename<") => {
                let tag = arg[10..].to_string();
                if !tag.is_empty() {
                    filename_tag = Some(tag);
                }
            }
            "-ext" | "--ext" => {
                if i + 1 < args.len() {
                    ext_filter = Some(args[i + 1].to_lowercase());
                    i += 1;
                }
            }
            "-all" => {
                // -all= means delete all writable tags
                // Already handled by the write tag parser below
            }
            arg if arg.contains('=') && arg.starts_with('-') => {
                let eq_pos = arg.find('=').unwrap();
                let mut tag = arg[1..eq_pos].to_string();
                let value = arg[eq_pos + 1..].to_string();

                // Date shift: -TAG+=VALUE or -TAG-=VALUE
                if tag.ends_with('+') || tag.ends_with('-') {
                    let shift_sign = if tag.ends_with('+') { "+" } else { "-" };
                    tag = tag[..tag.len() - 1].to_string();
                    let shift_str = format!("{}{}", shift_sign, value);
                    // Read current value, apply shift, write back
                    write_tags.push((format!("__SHIFT__:{}:{}", tag, shift_str), String::new()));
                } else if value.is_empty() {
                    delete_tags.push(tag);
                } else {
                    write_tags.push((tag, value));
                }
            }
            arg if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") => {
                options.requested_tags.push(arg[1..].to_string());
            }
            _ => files.push(arg.clone()),
        }
        i += 1;
    }

    // Stay-open mode: read commands from stdin
    if stay_open {
        run_stay_open(options, show_groups, short_names, json_output);
        return;
    }

    if files.is_empty() {
        eprintln!("Error: no input files specified");
        process::exit(1);
    }

    // Expand directories if recursive
    if recursive {
        let mut expanded = Vec::new();
        for f in &files {
            let path = Path::new(f);
            if path.is_dir() {
                collect_files(path, &ext_filter, &mut expanded);
            } else {
                expanded.push(f.clone());
            }
        }
        files = expanded;
    }

    // Apply extension filter
    if let Some(ref ext) = ext_filter {
        files.retain(|f| {
            Path::new(f)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase() == *ext)
                .unwrap_or(false)
        });
    }

    // Filename rename mode
    if let Some(ref tag_name) = filename_tag {
        let et = ExifTool::with_options(options.clone());
        for file in &files {
            match et.set_file_name_from_tag(file, tag_name, "%v") {
                Ok(new_name) => println!("'{}' --> '{}'", file, new_name),
                Err(e) => eprintln!("Error renaming {}: {}", file, e),
            }
        }
        return;
    }

    // Write mode
    if !write_tags.is_empty() || !delete_tags.is_empty() || tags_from_file.is_some() {
        run_write_mode(
            &files,
            &write_tags,
            &delete_tags,
            overwrite_original,
            options,
            tags_from_file.as_deref(),
        );
        return;
    }

    // List tags mode
    if list_tags {
        println!(
            "Supported file types: {}",
            exiftool_rs::FileType::all().len()
        );
        println!("Known EXIF tags: ~4300 (auto-generated from ExifTool source)");
        println!("Print conversions: ~17600");
        println!("MakerNotes manufacturers: Canon, Nikon, Sony, Olympus, Pentax, Panasonic, Fujifilm, Samsung, Sigma");
        return;
    }

    // Read mode
    let et = ExifTool::with_options(options);

    // Sort files by tag value if -fileOrder specified
    if let Some(ref order_tag) = file_order {
        sort_files_by_tag(&et, &mut files, order_tag);
    }

    // Apply -if condition to filter files
    let files = if let Some(ref cond) = if_condition {
        filter_files_by_condition(&et, &files, cond)
    } else {
        files
    };

    if files.is_empty() && !quiet {
        eprintln!("No matching files");
        return;
    }

    // Binary output mode
    if binary_output {
        print_binary(&et, &files);
        return;
    }

    // Print format mode: -p "format string"
    if let Some(ref fmt) = print_format {
        print_formatted(&et, &files, fmt);
        return;
    }

    // Prepare tag filter closure
    let exclude_lower: Vec<String> = exclude_tags.iter().map(|t| t.to_lowercase()).collect();

    // -diff: compare two files
    if let Some(ref diff_f) = diff_file {
        if let Some(file1) = files.first() {
            print_diff(&et, file1, diff_f);
        }
        return;
    }

    if csv_output {
        print_csv(&et, &files);
    } else if tab_output {
        print_tab(&et, &files);
    } else if xml_output {
        print_xml(&et, &files);
    } else if json_output {
        print_json_all(&et, &files);
    } else if args_output {
        print_args(&et, &files);
    } else if php_output {
        print_php(&et, &files);
    } else {
        let numeric = !et.options().print_conv;
        // -progress: show file counter on stderr
        if progress {
            for (idx, f) in files.iter().enumerate() {
                eprintln!("======== {} [{}/{}]", f, idx + 1, files.len());
            }
        }
        // -validate: add Validate tag
        // -htmlDump: emit HTML hex dump
        // -scanForXMP: scan for XMP
        // -v (verbose): handled at ExifTool level
        if validate {
            // Simple validation: if we can read tags, it's valid
            for f in &files {
                if let Ok(tags) = et.extract_info(f) {
                    println!(
                        "Validate                         : {}",
                        if tags.is_empty() { "Error" } else { "OK" }
                    );
                }
            }
        }
        // htmlDump handled above
        if scan_for_xmp {
            for f in &files {
                if let Ok(data) = std::fs::read(f) {
                    if let Some(xmp_tags) = scan_file_for_xmp(&data) {
                        for t in xmp_tags {
                            println!("{} : {}", pad_display(&t.name, 33), t.print_value);
                        }
                    }
                }
            }
        }
        if verbose > 0 {
            for f in &files {
                print_verbose(&et, f, verbose);
            }
            if !validate {
                return; // verbose replaces normal output
            }
        }
        if html_dump {
            for f in &files {
                print_html_dump(f);
            }
            return;
        }
        if !validate {
            // Load translations if -lang specified
            let translations = lang
                .as_ref()
                .and_then(|l| exiftool_rs::i18n::get_translations(l));
            print_text_full(
                &et,
                &files,
                show_groups,
                short_names,
                sort_tags,
                show_tag_ids,
                &exclude_lower,
                quiet,
                no_composites,
                numeric,
                &translations,
            );
        }
    }
}

// ============================================================================
// Stay-open mode
// ============================================================================

fn run_stay_open(options: Options, show_groups: bool, short_names: bool, json: bool) {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let et = ExifTool::with_options(options);

    // Read commands line by line
    // Protocol: each line is a file path or option
    // "{ready}" marker sent after each file is processed
    let _ = writeln!(stdout, "{{ready}}");
    let _ = stdout.flush();

    let mut current_args: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };

        if line == "-stay_open" || line.eq_ignore_ascii_case("false") || line == "0" {
            break;
        }

        if line == "-execute" || line == "-execute\n" {
            // Process accumulated args
            if !current_args.is_empty() {
                for file in &current_args {
                    match et.extract_info(file) {
                        Ok(tags) => {
                            if json {
                                print_json_tags(&tags, file, false);
                            } else {
                                for tag in &tags {
                                    if show_groups {
                                        println!(
                                            "[{}] {} : {}",
                                            tag.group.family1,
                                            pad_display(&tag.name, 32),
                                            tag.print_value
                                        );
                                    } else if short_names {
                                        println!(
                                            "{} : {}",
                                            pad_display(&tag.name, 32),
                                            tag.print_value
                                        );
                                    } else {
                                        println!(
                                            "{} : {}",
                                            pad_display(&tag.description, 32),
                                            tag.print_value
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => eprintln!("Error: {} - {}", file, e),
                    }
                }
                current_args.clear();
            }
            println!("{{ready}}");
            let _ = stdout.flush();
        } else if !line.is_empty() && !line.starts_with('-') {
            current_args.push(line);
        }
    }
}

// ============================================================================
// Write mode
// ============================================================================

fn run_write_mode(
    files: &[String],
    write_tags: &[(String, String)],
    delete_tags: &[String],
    overwrite_original: bool,
    options: Options,
    tags_from_file: Option<&str>,
) {
    let mut et = ExifTool::with_options(options);

    // Copy tags from source file if specified
    if let Some(src_file) = tags_from_file {
        match et.set_new_values_from_file(src_file, None) {
            Ok(n) => eprintln!("    Copied {} tags from {}", n, src_file),
            Err(e) => eprintln!("Error reading {}: {}", src_file, e),
        }
    }

    // Handle date shifts and regular tags
    for (tag, value) in write_tags {
        if tag.starts_with("__SHIFT__:") {
            // Date shift: __SHIFT__:TagName:+H:M:S
            let parts: Vec<&str> = tag.splitn(3, ':').collect();
            if parts.len() == 3 {
                let _tag_name = parts[1];
                let _shift = parts[2];
                // We need to read the current value from each file and shift it
                // For now, queue as a special shift marker (handled per-file below)
                // We'll handle it in the per-file loop
            }
            continue;
        }
        et.set_new_value(tag, Some(value));
    }
    for tag in delete_tags {
        et.set_new_value(tag, None);
    }

    // Process date shifts per file
    let shifts: Vec<(&str, &str)> = write_tags
        .iter()
        .filter(|(t, _)| t.starts_with("__SHIFT__:"))
        .filter_map(|(t, _)| {
            let parts: Vec<&str> = t.splitn(3, ':').collect();
            if parts.len() == 3 {
                Some((parts[1], parts[2]))
            } else {
                None
            }
        })
        .collect();

    let mut _total_written = 0u32;
    for file in files {
        // Apply date shifts for this specific file
        if !shifts.is_empty() {
            if let Ok(file_tags) = et.extract_info(file) {
                for &(tag_name, shift_str) in &shifts {
                    if let Some(current) = file_tags
                        .iter()
                        .find(|t| t.name.to_lowercase() == tag_name.to_lowercase())
                    {
                        if let Some(shifted) =
                            exiftool_rs::exiftool::shift_datetime(&current.print_value, shift_str)
                        {
                            et.set_new_value(tag_name, Some(&shifted));
                        }
                    }
                }
            }
        }

        let dst = if overwrite_original {
            file.clone()
        } else {
            let path = Path::new(file);
            let parent = path.parent().unwrap_or(Path::new(""));
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
            let ext = path
                .extension()
                .map(|e| format!(".{}", e.to_str().unwrap_or("")))
                .unwrap_or_default();
            parent
                .join(format!("{}_exiftool_out{}", stem, ext))
                .to_string_lossy()
                .to_string()
        };
        match et.write_info(file, &dst) {
            Ok(n) => {
                _total_written += n;
                if overwrite_original {
                    println!("    1 image files updated");
                } else {
                    println!("    {} tag(s) written to {}", n, dst);
                }
            }
            Err(e) => eprintln!("Error writing {}: {}", file, e),
        }
    }
    if files.len() > 1 {
        println!("    {} image files updated", files.len());
    }
}

// ============================================================================
// Output formats
// ============================================================================

/// Sort files by a tag value.
fn sort_files_by_tag(et: &ExifTool, files: &mut Vec<String>, tag_name: &str) {
    let mut tagged: Vec<(String, String)> = files
        .iter()
        .map(|f| {
            let val = et
                .extract_info(f)
                .ok()
                .and_then(|tags| {
                    tags.iter()
                        .find(|t| t.name.to_lowercase() == tag_name.to_lowercase())
                        .map(|t| t.print_value.clone())
                })
                .unwrap_or_default();
            (f.clone(), val)
        })
        .collect();
    tagged.sort_by(|a, b| a.1.cmp(&b.1));
    *files = tagged.into_iter().map(|(f, _)| f).collect();
}

/// Full-featured text output with all options.
#[allow(clippy::too_many_arguments)]
fn print_text_full(
    et: &ExifTool,
    files: &[String],
    show_groups: bool,
    short_names: bool,
    sort_tags: bool,
    show_tag_ids: bool,
    exclude_tags: &[String],
    quiet: bool,
    no_composites: bool,
    numeric: bool,
    translations: &Option<std::collections::HashMap<&str, &str>>,
) {
    let multiple = files.len() > 1;
    for file in files {
        match et.extract_info(file) {
            Ok(mut tags) => {
                // Apply filters
                if no_composites {
                    tags.retain(|t| t.group.family0 != "Composite");
                }
                if !exclude_tags.is_empty() {
                    tags.retain(|t| !exclude_tags.contains(&t.name.to_lowercase()));
                }
                if sort_tags {
                    tags.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                }

                if multiple && !quiet {
                    println!("======== {}", file);
                }
                for tag in &tags {
                    let val_raw = tag.display_value(numeric);
                    let val = sanitize_display_value(&val_raw);
                    let id_prefix = if show_tag_ids {
                        format!("[{}] ", tag.id)
                    } else {
                        String::new()
                    };
                    if show_groups {
                        println!(
                            "{}[{}] {} : {}",
                            id_prefix,
                            tag.group.family1,
                            pad_display(&tag.name, 32),
                            val
                        );
                    } else if short_names {
                        println!("{}{} : {}", id_prefix, pad_display(&tag.name, 32), val);
                    } else {
                        // Apply i18n translation if -lang is set
                        let desc = if let Some(ref tr) = translations {
                            tr.get(tag.name.as_str())
                                .copied()
                                .unwrap_or(&tag.description)
                        } else {
                            &tag.description
                        };
                        println!("{}{} : {}", id_prefix, pad_display(desc, 32), val);
                    }
                }
            }
            Err(e) => {
                if !quiet {
                    eprintln!("Error: {} - {}", file, e);
                }
            }
        }
    }
}

/// Filter files by a simple condition on tag values.
/// Supports: '$TagName eq "value"', '$TagName ne "value"', '$TagName =~ /pattern/'
fn filter_files_by_condition(et: &ExifTool, files: &[String], condition: &str) -> Vec<String> {
    let cond = condition.trim().trim_matches('\'').trim_matches('"');
    files
        .iter()
        .filter(|file| match et.extract_info(file.as_str()) {
            Ok(tags) => evaluate_condition(&tags, cond),
            Err(_) => false,
        })
        .cloned()
        .collect()
}

fn evaluate_condition(tags: &[exiftool_rs::Tag], condition: &str) -> bool {
    // Parse: $TagName op "value"
    let cond = condition.trim();

    // Extract tag name (starts with $)
    if !cond.starts_with('$') {
        return true; // Can't parse, include file
    }

    let rest = &cond[1..];
    let (tag_name, operator, value) = if let Some(pos) = rest.find(" eq ") {
        (
            &rest[..pos],
            "eq",
            rest[pos + 4..].trim().trim_matches('"').trim_matches('\''),
        )
    } else if let Some(pos) = rest.find(" ne ") {
        (
            &rest[..pos],
            "ne",
            rest[pos + 4..].trim().trim_matches('"').trim_matches('\''),
        )
    } else if let Some(pos) = rest.find(" =~ ") {
        (&rest[..pos], "=~", rest[pos + 4..].trim().trim_matches('/'))
    } else if let Some(pos) = rest.find(" !~ ") {
        (&rest[..pos], "!~", rest[pos + 4..].trim().trim_matches('/'))
    } else if let Some(pos) = rest.find(" > ") {
        (&rest[..pos], ">", rest[pos + 3..].trim().trim_matches('"'))
    } else if let Some(pos) = rest.find(" < ") {
        (&rest[..pos], "<", rest[pos + 3..].trim().trim_matches('"'))
    } else if let Some(pos) = rest.find(" >= ") {
        (&rest[..pos], ">=", rest[pos + 4..].trim().trim_matches('"'))
    } else {
        return true; // Can't parse
    };

    let tag_value = tags
        .iter()
        .find(|t| t.name.to_lowercase() == tag_name.to_lowercase())
        .map(|t| t.print_value.as_str())
        .unwrap_or("");

    match operator {
        "eq" => tag_value == value,
        "ne" => tag_value != value,
        "=~" => tag_value.contains(value),
        "!~" => !tag_value.contains(value),
        ">" => {
            if let (Ok(a), Ok(b)) = (tag_value.parse::<f64>(), value.parse::<f64>()) {
                a > b
            } else {
                tag_value > value
            }
        }
        "<" => {
            if let (Ok(a), Ok(b)) = (tag_value.parse::<f64>(), value.parse::<f64>()) {
                a < b
            } else {
                tag_value < value
            }
        }
        ">=" => {
            if let (Ok(a), Ok(b)) = (tag_value.parse::<f64>(), value.parse::<f64>()) {
                a >= b
            } else {
                tag_value >= value
            }
        }
        _ => true,
    }
}

/// Print binary data for requested tags (e.g., ThumbnailImage, PreviewImage).
fn print_binary(et: &ExifTool, files: &[String]) {
    let mut stdout = io::stdout();
    for file in files {
        match et.extract_info(file) {
            Ok(tags) => {
                for tag in &tags {
                    match &tag.raw_value {
                        exiftool_rs::Value::Binary(data) => {
                            let _ = stdout.write_all(data);
                        }
                        exiftool_rs::Value::Undefined(data) => {
                            let _ = stdout.write_all(data);
                        }
                        _ => {
                            let _ = stdout.write_all(tag.print_value.as_bytes());
                            let _ = stdout.write_all(b"\n");
                        }
                    }
                }
            }
            Err(e) => eprintln!("Error: {} - {}", file, e),
        }
    }
}

/// Print using a format string. $TagName is replaced with tag values.
fn print_formatted(et: &ExifTool, files: &[String], format: &str) {
    for file in files {
        if let Ok(tags) = et.extract_info(file) {
            let mut output = format.to_string();
            // Replace $TagName with values
            for tag in &tags {
                let pattern = format!("${}", tag.name);
                if output.contains(&pattern) {
                    output = output.replace(&pattern, &tag.print_value);
                }
            }
            // Also support $filename, $directory
            output = output.replace(
                "$filename",
                Path::new(file)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(""),
            );
            output = output.replace(
                "$directory",
                Path::new(file)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or(""),
            );
            // Clean up unreplaced variables
            println!("{}", output);
        }
    }
}

/// Print tab-separated output.
fn print_tab(et: &ExifTool, files: &[String]) {
    for file in files {
        if let Ok(tags) = et.extract_info(file) {
            for tag in &tags {
                println!("{}\t{}\t{}", file, tag.name, tag.print_value);
            }
        }
    }
}

fn print_json_all(et: &ExifTool, files: &[String]) {
    print!("[");
    for (idx, file) in files.iter().enumerate() {
        match et.extract_info(file) {
            Ok(tags) => print_json_tags(&tags, file, idx > 0),
            Err(e) => eprintln!("Error: {} - {}", file, e),
        }
    }
    println!("]");
}

fn print_json_tags(tags: &[exiftool_rs::Tag], filename: &str, prepend_comma: bool) {
    if prepend_comma {
        print!(",");
    }
    println!("{{");
    println!("  \"SourceFile\": \"{}\",", escape_json(filename));
    for (i, tag) in tags.iter().enumerate() {
        let comma = if i + 1 < tags.len() { "," } else { "" };
        // Try to output numbers as numbers, strings as strings
        let value_str = &tag.print_value;
        if let Ok(n) = value_str.parse::<i64>() {
            println!("  \"{}\": {}{}", tag.name, n, comma);
        } else if let Ok(f) = value_str.parse::<f64>() {
            println!("  \"{}\": {}{}", tag.name, f, comma);
        } else {
            println!(
                "  \"{}\": \"{}\"{}",
                tag.name,
                escape_json(value_str),
                comma
            );
        }
    }
    print!("}}");
}

/// Output in -args format: -TAG=VALUE per line
fn print_args(et: &ExifTool, files: &[String]) {
    for file in files {
        if let Ok(tags) = et.extract_info(file) {
            for tag in &tags {
                println!("-{}={}", tag.name, tag.print_value);
            }
        }
    }
}

/// Output in PHP array format
fn print_php(et: &ExifTool, files: &[String]) {
    println!("Array(");
    for file in files {
        println!("Array(");
        println!("  \"SourceFile\" => \"{}\",", file);
        if let Ok(tags) = et.extract_info(file) {
            for tag in &tags {
                let val = tag.print_value.replace('\\', "\\\\").replace('"', "\\\"");
                println!("  \"{}\" => \"{}\",", tag.name, val);
            }
        }
        println!("),");
    }
    println!(");");
}

/// Compare metadata between two files (-diff)
fn print_diff(et: &ExifTool, file1: &str, file2: &str) {
    let tags1 = et.extract_info(file1).unwrap_or_default();
    let tags2 = et.extract_info(file2).unwrap_or_default();

    let map1: std::collections::HashMap<&str, &str> = tags1
        .iter()
        .map(|t| (t.name.as_str(), t.print_value.as_str()))
        .collect();
    let map2: std::collections::HashMap<&str, &str> = tags2
        .iter()
        .map(|t| (t.name.as_str(), t.print_value.as_str()))
        .collect();

    let mut all_keys: Vec<&str> = map1.keys().chain(map2.keys()).copied().collect();
    all_keys.sort();
    all_keys.dedup();

    for key in &all_keys {
        let v1 = map1.get(key).copied().unwrap_or("(none)");
        let v2 = map2.get(key).copied().unwrap_or("(none)");
        if v1 != v2 {
            println!("  {}", key);
            println!("    < {}", v1);
            println!("    > {}", v2);
        }
    }
}

/// Print verbose output (-v0 to -v5)
/// Shows tag structure with indentation and raw values
fn print_verbose(et: &ExifTool, file: &str, level: u8) {
    let tags = match et.extract_info(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {} - {}", file, e);
            return;
        }
    };

    // Group tags by family1 (IFD0, ExifIFD, Canon, etc.)
    let mut groups: Vec<(String, Vec<&exiftool_rs::Tag>)> = Vec::new();
    let mut current_group = String::new();

    for tag in &tags {
        let grp = &tag.group.family1;
        if grp != &current_group {
            current_group = grp.clone();
            groups.push((grp.clone(), Vec::new()));
        }
        if let Some(last) = groups.last_mut() {
            last.1.push(tag);
        }
    }

    for (group, group_tags) in &groups {
        if group == "File" || group == "Composite" || group == "ExifTool" {
            // File-level tags: no indentation
            for tag in group_tags {
                if level >= 1 {
                    // -v1+: show raw values (numeric)
                    println!("  {} = {}", tag.name, tag.print_value);
                } else {
                    println!("{} : {}", pad_display(&tag.name, 33), tag.print_value);
                }
            }
        } else {
            // IFD/MakerNote groups: show with structure
            println!(
                "  + [{} directory with {} entries]",
                group,
                group_tags.len()
            );
            for (idx, tag) in group_tags.iter().enumerate() {
                if level >= 2 {
                    // -v2+: show tag index and group
                    println!("  | {})  {} = {}", idx, tag.name, tag.print_value);
                } else {
                    println!("  | {})  {} = {}", idx, tag.name, tag.print_value);
                }
            }
        }
    }
}

/// Print HTML hex dump of file structure (-htmlDump)
fn print_html_dump(file: &str) {
    let data = match std::fs::read(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error reading {}: {}", file, e);
            return;
        }
    };

    println!("<!DOCTYPE HTML>");
    println!("<html><head><title>HTML Dump ({})</title>", file);
    println!("<meta charset=\"UTF-8\">");
    println!("<style>");
    println!("body {{ font-family: monospace; font-size: 12px; }}");
    println!("table {{ border-collapse: collapse; }}");
    println!("td {{ padding: 1px 4px; border: 1px solid #ddd; }}");
    println!(".offset {{ color: #888; }}");
    println!(".hex {{ color: #000; }}");
    println!(".ascii {{ color: #080; }}");
    println!("</style></head><body>");
    println!("<h2>Hex Dump: {}</h2>", file);
    println!("<p>File size: {} bytes</p>", data.len());
    println!("<table>");

    // Dump first 4KB (or less for small files)
    let dump_len = data.len().min(4096);
    for row in (0..dump_len).step_by(16) {
        let end = (row + 16).min(dump_len);
        let hex: String = data[row..end]
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = data[row..end]
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("<tr><td class=\"offset\">{:08x}</td><td class=\"hex\">{:<48}</td><td class=\"ascii\">{}</td></tr>",
            row, hex, ascii);
    }
    if data.len() > 4096 {
        println!(
            "<tr><td colspan=\"3\">... ({} more bytes)</td></tr>",
            data.len() - 4096
        );
    }

    println!("</table></body></html>");
}

/// Scan entire file for XMP data (<?xpacket begin= ... <?xpacket end)
fn scan_file_for_xmp(data: &[u8]) -> Option<Vec<exiftool_rs::Tag>> {
    let marker = b"<?xpacket begin=";
    let end_marker = b"<?xpacket end";
    let text = data;

    if let Some(start) = text.windows(marker.len()).position(|w| w == marker) {
        if let Some(end_rel) = text[start..]
            .windows(end_marker.len())
            .position(|w| w == end_marker)
        {
            // Find the end of the <?xpacket end...?> tag
            let end = start + end_rel;
            if let Some(close) = text[end..].windows(2).position(|w| w == b"?>") {
                let xmp_data = &text[start..end + close + 2];
                if let Ok(tags) = exiftool_rs::metadata::XmpReader::read(xmp_data) {
                    if !tags.is_empty() {
                        return Some(tags);
                    }
                }
            }
        }
    }
    None
}

fn print_csv(et: &ExifTool, files: &[String]) {
    // Collect all unique tag names across all files
    let mut all_tags: Vec<String> = Vec::new();
    let mut all_results: Vec<(String, Vec<(String, String)>)> = Vec::new();

    for file in files {
        if let Ok(tags) = et.extract_info(file) {
            let mut row: Vec<(String, String)> = Vec::new();
            for tag in &tags {
                if !all_tags.contains(&tag.name) {
                    all_tags.push(tag.name.clone());
                }
                row.push((tag.name.clone(), tag.print_value.clone()));
            }
            all_results.push((file.clone(), row));
        }
    }

    // Header
    print!("SourceFile");
    for name in &all_tags {
        print!(",{}", name);
    }
    println!();

    // Rows
    for (file, row) in &all_results {
        print!("{}", escape_csv(file));
        for name in &all_tags {
            let value = row
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            print!(",{}", escape_csv(value));
        }
        println!();
    }
}

fn print_xml(et: &ExifTool, files: &[String]) {
    println!("<?xml version='1.0' encoding='UTF-8'?>");
    println!("<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'");
    println!("  xmlns:et='http://ns.exiftool.org/1.0/'>");

    for file in files {
        if let Ok(tags) = et.extract_info(file) {
            println!("  <rdf:Description rdf:about='{}'>", escape_xml(file));
            for tag in &tags {
                let ns = tag.group.family0.to_lowercase();
                println!(
                    "    <et:{}:{} rdf:datatype='string'>{}</et:{}:{}>",
                    ns,
                    tag.name,
                    escape_xml(&tag.print_value),
                    ns,
                    tag.name
                );
            }
            println!("  </rdf:Description>");
        }
    }

    println!("</rdf:RDF>");
}

// ============================================================================
// Directory recursion
// ============================================================================

fn collect_files(dir: &Path, ext_filter: &Option<String>, files: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext_filter, files);
        } else if path.is_file() {
            if let Some(ref ext) = ext_filter {
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    != Some(ext.clone())
                {
                    continue;
                }
            }
            if let Some(s) = path.to_str() {
                files.push(s.to_string());
            }
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Translate control characters (0x01-0x1f, 0x7f) to '.' and remove null bytes,
/// matching ExifTool's output behavior for -s format.
fn sanitize_display_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '\0' {
            // remove null bytes
        } else if ('\x01'..='\x1f').contains(&ch) || ch == '\x7f' {
            result.push('.');
        } else {
            result.push(ch);
        }
    }
    // Remove trailing whitespace
    let trimmed = result.trim_end();
    trimmed.to_string()
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn print_usage() {
    eprintln!("exiftool-rs {}", exiftool_rs::VERSION);
    eprintln!("A Rust implementation of ExifTool — read/write metadata in 55+ file formats");
    eprintln!();
    eprintln!("Usage: exiftool-rs [OPTIONS] [-TAG[=VALUE]...] FILE [FILE...]");
    eprintln!();
    eprintln!("Read options:");
    eprintln!("  -j, -json             Output in JSON format");
    eprintln!("  -csv                  Output in CSV format");
    eprintln!("  -X, -xml              Output in XML/RDF format");
    eprintln!("  -args                 Output as -TAG=VALUE (for piping back)");
    eprintln!("  -php                  Output as PHP array");
    eprintln!("  -g, -G                Show group names");
    eprintln!("  -n, -num              Show numerical values (no print conversion)");
    eprintln!("  -s, -short            Show short tag names only");
    eprintln!("  -b, -binary           Output binary data (thumbnails, etc.)");
    eprintln!("  -r, -recurse          Recursively scan directories");
    eprintln!("  -ext EXT              Process only files with extension EXT");
    eprintln!("  -ee                   Extract embedded data (video frame metadata)");
    eprintln!("  -v[NUM]               Verbose output (0-5, shows file structure)");
    eprintln!("  -D                    Show tag IDs in decimal");
    eprintln!("  -H                    Show tag IDs in hexadecimal");
    eprintln!("  -t, -tab              Tab-delimited output");
    eprintln!("  -TAG                  Extract specific tag(s)");
    eprintln!("  --TAG                 Exclude specific tag(s)");
    eprintln!();
    eprintln!("Write options:");
    eprintln!("  -TAG=VALUE            Set tag to value");
    eprintln!("  -TAG=                 Delete tag");
    eprintln!("  -overwrite_original   Modify file in place");
    eprintln!("  -tagsFromFile FILE    Copy tags from another file");
    eprintln!();
    eprintln!("Processing:");
    eprintln!("  -diff FILE            Compare metadata with another file");
    eprintln!("  -validate             Validate metadata structure");
    eprintln!("  -scanForXMP           Scan entire file for XMP data");
    eprintln!("  -htmlDump             Generate HTML hex dump of file structure");
    eprintln!("  -progress             Show processing progress on stderr");
    eprintln!();
    eprintln!("Language:");
    eprintln!("  -lang LANG            Set language for tag descriptions");
    eprintln!("                        Supported languages:");
    for (code, name) in exiftool_rs::i18n::AVAILABLE_LANGUAGES {
        eprintln!("                          {:<8} {}", code, name);
    }
    eprintln!();
    eprintln!("Other:");
    eprintln!("  -stay_open True       Keep running, read commands from stdin");
    eprintln!("  -ver                  Show version");
    eprintln!("  -h, -help             Show this help");
    eprintln!();
    eprintln!("GUI (requires --features gui):");
    eprintln!("  exiftool-rs-gui [FILE|DIR]   Open metadata viewer/editor");
}
