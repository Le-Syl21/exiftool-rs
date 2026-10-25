//! Image::ExifTool::Fixup (Image/ExifTool/Fixup.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Utility to handle pointer fixups. A Fixup stays the blessed hash of the
//! Perl: Start, Shift, Fixups (sub-Fixup objects) and Pointers (arrays of
//! pointer offsets keyed by byte order, "2" for 16-bit pointers, and
//! "_$marker" for tagged pointers).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{bless, print, ref_name, substr};
use crate::perl::Value;
use crate::pl::{self, arg, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn get_byte_order(it: &mut Interp) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetByteOrder", vec![])
}

fn set_byte_order(it: &mut Interp, o: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![o])
}

/// `/_$marker$/` (the marker is interpolated as a pattern, as in the Perl)
fn marker_re(marker: &Value, prefix: &[u8]) -> ER<pl::Re> {
    let mut p = prefix.to_vec();
    p.push(b'_');
    p.extend_from_slice(&marker.to_bytes());
    p.push(b'$');
    pl::re_dyn(&p, "")
}

fn new_impl(it: &mut Interp, that: &Value) -> ER<Value> {
    // local $_;
    let saved_us = it.get_scalar("main::_");
    it.set_scalar("main::_", Value::Undef);
    let r = ref_name(that);
    let class = if r.truthy() {
        r
    } else if that.truthy() {
        that.clone()
    } else {
        pl::s("Image::ExifTool::Fixup")
    };
    let self_ = Value::new_hash();
    bless(&self_, &class);

    // initialize required members
    self_.set("Start", n(0));
    self_.set("Shift", n(0));

    it.set_scalar("main::_", saved_us);
    Ok(self_)
}

/// New - create new Fixup object
/// Inputs: 0) reference to Fixup object or Fixup class name
pub fn new(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(new_impl(it, &arg(&a, 0))?)
}

/// Clone this object
/// Inputs: 0) reference to Fixup object or Fixup class name
/// Returns: reference to new Fixup object
pub fn clone(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let clone = pl::method(it, &pl::s("Image::ExifTool::Fixup"), "new", vec![])?;
    clone.set("Start", self_.get("Start"));
    clone.set("Shift", self_.get("Shift"));
    let phash = self_.get("Pointers");
    if phash.truthy() {
        clone.set("Pointers", Value::new_hash());
        for byte_order in phash.keys() {
            let pointers = phash.get(&byte_order).items();
            clone.get("Pointers").set(&byte_order, Value::new_array(pointers));
        }
    }
    if self_.get("Fixups").truthy() {
        clone.set("Fixups", Value::new_array(vec![]));
        for sub_fixup in self_.get("Fixups").items() {
            let c = pl::method(it, &sub_fixup, "Clone", vec![])?;
            clone.get_or_new_array("Fixups").push(c);
        }
    }
    ret(clone)
}

/// Add fixup pointer or another fixup object below this one
/// Inputs: 0) Fixup object reference
///         1) Scalar for pointer offset, or reference to Fixup object
///         2) Optional marker name for the pointer
///         3) Optional pointer format ('int16u' or 'int32u', defaults to 'int32u')
/// Notes: Byte ordering must be set properly for the pointer being added (must keep
/// track of the byte order of each offset since MakerNotes may have different byte order!)
pub fn add_fixup(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, pointer, marker, format) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if pointer.is_ref() {
        if !self_.get("Fixups").truthy() {
            self_.set("Fixups", Value::new_array(vec![]));
        }
        let f = self_.get_or_new_array("Fixups");
        f.push(pointer);
        ret(n(f.len() as i64))
    } else {
        let mut byte_order = get_byte_order(it)?;
        if !format.is_undef() {
            if format.eq_str("int16u") {
                byte_order = pl::concat(&byte_order, &pl::s("2"));
            } else if !format.eq_str("int32u") {
                pl::warn(it, &format!("Bad Fixup pointer format {}\n", format.to_text()))?;
            }
        }
        if !marker.is_undef() {
            byte_order = pl::concat(&byte_order, &pl::concat(&pl::s("_"), &marker));
        }
        let mut phash = self_.get("Pointers");
        if !phash.truthy() {
            phash = Value::new_hash();
            self_.set("Pointers", phash.clone());
        }
        let key = byte_order.to_bytes();
        if !phash.get(&key).truthy() {
            phash.set(&key, Value::new_array(vec![]));
        }
        let arr = phash.get_or_new_array(&key);
        arr.push(pointer);
        ret(n(arr.len() as i64))
    }
}

fn apply_fixup_impl(it: &mut Interp, self_: &Value, data_pt: &Value) -> ER<Value> {
    let start = self_.get("Start");
    let shift = pl::add(&self_.get("Shift"), &start); // make shift relative to start
    let mut phash = self_.get("Pointers");

    // fix up pointers in this fixup
    if phash.truthy() && (start.truthy() || shift.truthy()) {
        let save_order = get_byte_order(it)?; // save original byte ordering
        for byte_order in phash.keys() {
            let bo = Value::from(byte_order.clone());
            let o = substr(it, &bo, &n(0), Some(&n(2)))?;
            set_byte_order(it, o)?;
            // apply the fixup offset shift (must get as signed integer
            // to avoid overflow in case it was negative before)
            let (get, set) = if pl::re(r"^(II2|MM2)", "").is_match(&bo) {
                ("Image::ExifTool::Get16s", "Image::ExifTool::Set16u")
            } else {
                ("Image::ExifTool::Get32s", "Image::ExifTool::Set32u")
            };
            let arr = phash.get(&byte_order);
            for i in 0..arr.len() {
                let ptr = pl::add(&arr.at(i as i64), &start); // update pointer to new start location
                arr.set_at(i, ptr.clone());
                if !shift.truthy() {
                    continue;
                }
                let v = pl::call(it, get, vec![data_pt.clone(), ptr.clone()])?;
                pl::call(it, set, vec![pl::add(&v, &shift), data_pt.clone(), ptr])?;
            }
        }
        set_byte_order(it, save_order)?; // restore original byte ordering
    }
    // recurse into contained fixups
    if self_.get("Fixups").truthy() {
        // create our pointer hash if it doesn't exist
        if !phash.truthy() {
            phash = Value::new_hash();
            self_.set("Pointers", phash.clone());
        }
        // loop through all contained fixups
        for sub_fixup in self_.get("Fixups").items() {
            // adjust the subfixup start and shift
            sub_fixup.set("Start", pl::add(&sub_fixup.get("Start"), &start));
            sub_fixup.set("Shift", pl::add(&sub_fixup.get("Shift"), &pl::sub(&shift, &start)));
            // recursively apply contained fixups
            apply_fixup_impl(it, &sub_fixup, data_pt)?;
            let shash = sub_fixup.get("Pointers");
            if !shash.truthy() {
                continue;
            }
            // add all pointers to our collapsed lists
            for byte_order in shash.keys() {
                if !phash.get(&byte_order).truthy() {
                    phash.set(&byte_order, Value::new_array(vec![]));
                }
                let dst = phash.get(&byte_order);
                for p in shash.get(&byte_order).items() {
                    dst.push(p);
                }
                shash.delete(&byte_order);
            }
            sub_fixup.delete("Pointers");
        }
        self_.delete("Fixups"); // remove our contained fixups
    }
    // reset our Start/Shift for the collapsed fixup
    self_.set("Shift", n(0));
    self_.set("Start", n(0));
    Ok(n(0))
}

/// fix up pointer offsets
/// Inputs: 0) Fixup object reference, 1) data reference
/// Outputs: Collapses fixup hierarchy into linear lists of fixup pointers
pub fn apply_fixup(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(apply_fixup_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// Is this Fixup empty?
/// Inputs: 0) Fixup object ref
/// Returns: True if there are no offsets to fix
pub fn is_empty(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let phash = self_.get("Pointers");
    if phash.truthy() {
        for key in phash.keys() {
            let v = phash.get(&key);
            if !v.is_array() {
                continue;
            }
            if !v.is_empty() {
                return ret(n(0));
            }
        }
    }
    ret(n(1))
}

/// Does specified marker exist?
/// Inputs: 0) Fixup object reference, 1) marker name
/// Returns: True if fixup contains specified marker name
pub fn has_marker(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, marker) = (arg(&a, 0), arg(&a, 1));
    let phash = self_.get("Pointers");
    if !phash.truthy() {
        return ret(n(0));
    }
    let r = marker_re(&marker, b"")?;
    if phash.keys().into_iter().any(|k| r.is_match(&Value::from(k))) {
        return ret(n(1));
    }
    if !self_.get("Fixups").truthy() {
        return ret(n(0));
    }
    for sub_fixup in self_.get("Fixups").items() {
        if pl::method(it, &sub_fixup, "HasMarker", vec![marker.clone()])?.truthy() {
            return ret(n(1));
        }
    }
    ret(n(0))
}

/// Set all marker pointers to specified value
/// Inputs: 0) Fixup object reference, 1) data reference
///         2) marker name, 3) pointer value, 4) offset to start of data
pub fn set_marker_pointers(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (self_, data_pt, marker, value, start_offset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let start = pl::add(&self_.get("Start"), &if start_offset.truthy() { start_offset } else { n(0) });
    let phash = self_.get("Pointers");

    if phash.truthy() {
        let save_order = get_byte_order(it)?; // save original byte ordering
        let r = marker_re(&marker, b"^(II|MM)(2?)")?;
        for byte_order in phash.keys() {
            let Some(m) = r.m(&Value::from(byte_order.clone())) else { continue };
            set_byte_order(it, pl::cap(&m, 1))?;
            let set = if pl::cap(&m, 2).truthy() { "Image::ExifTool::Set16u" } else { "Image::ExifTool::Set32u" };
            for ptr in phash.get(&byte_order).items() {
                pl::call(it, set, vec![value.clone(), data_pt.clone(), pl::add(&ptr, &start)])?;
            }
        }
        set_byte_order(it, save_order)?; // restore original byte ordering
    }
    if self_.get("Fixups").truthy() {
        for sub_fixup in self_.get("Fixups").items() {
            pl::method(it, &sub_fixup, "SetMarkerPointers", vec![data_pt.clone(), marker.clone(), value.clone(), start.clone()])?;
        }
    }
    pl::ret_nothing(w)
}

/// Get pointer values for specified marker
/// Inputs: 0) Fixup object reference, 1) data reference,
///         2) marker name, 3) offset to start of data
/// Returns: List of marker pointers in list context, or first marker pointer otherwise
pub fn get_marker_pointers(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (self_, data_pt, marker, start_offset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let start = pl::add(&self_.get("Start"), &if start_offset.truthy() { start_offset } else { n(0) });
    let phash = self_.get("Pointers");
    let mut pointers: Vec<Value> = Vec::new();

    if phash.truthy() {
        let save_order = get_byte_order(it)?;
        let r = marker_re(&marker, b"")?;
        let keys: Vec<Value> = phash.keys().into_iter().map(Value::from).filter(|k| r.is_match(k)).collect();
        for byte_order in keys {
            let o = substr(it, &byte_order, &n(0), Some(&n(2)))?;
            set_byte_order(it, o)?;
            let get = if pl::re(r"^(II2|MM2)", "").is_match(&byte_order) { "Image::ExifTool::Get16u" } else { "Image::ExifTool::Get32u" };
            for ptr in phash.get(byte_order.to_bytes()).items() {
                pointers.push(pl::call(it, get, vec![data_pt.clone(), pl::add(&ptr, &start)])?);
            }
        }
        set_byte_order(it, save_order)?; // restore original byte ordering
    }
    if self_.get("Fixups").truthy() {
        for sub_fixup in self_.get("Fixups").items() {
            pointers.extend(pl::method_list(it, &sub_fixup, "GetMarkerPointers", vec![data_pt.clone(), marker.clone(), start.clone()])?);
        }
    }
    if w == Want::List {
        return Ok(pointers);
    }
    ret(pointers.into_iter().next().unwrap_or_default())
}

fn dump_impl(it: &mut Interp, self_: &Value, indent: &Value) -> ER<()> {
    let indent = if indent.truthy() { indent.clone() } else { pl::s("") };
    let fmt = pl::concat(&indent, &pl::s("Fixup start=0x%x shift=0x%x\n"));
    let line = pl::sprintf_v(&fmt, &[self_.get("Start"), self_.get("Shift")]);
    print(it, line)?;
    let phash = self_.get("Pointers");
    if phash.truthy() {
        let mut keys = phash.keys();
        keys.sort();
        for byte_order in keys {
            let joined: Vec<Vec<u8>> = phash.get(&byte_order).items().iter().map(|v| v.to_bytes().to_vec()).collect();
            let mut line = indent.to_bytes().to_vec();
            line.extend_from_slice(b"  ");
            line.extend_from_slice(&byte_order);
            line.extend_from_slice(b": ");
            line.extend_from_slice(&joined.join(&b' '));
            line.push(b'\n');
            print(it, Value::bytes(line))?;
        }
    }
    if self_.get("Fixups").truthy() {
        for sub_fixup in self_.get("Fixups").items() {
            dump_impl(it, &sub_fixup, &pl::concat(&indent, &pl::s("  ")))?;
        }
    }
    Ok(())
}

/// Dump fixup to console for debugging
/// Inputs: 0) Fixup object reference, 1) optional initial indent string
pub fn dump(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    dump_impl(it, &arg(&a, 0), &arg(&a, 1))?;
    pl::ret_nothing(w)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Fixup::new", new);
    it.define_native("Image::ExifTool::Fixup::Clone", clone);
    it.define_native("Image::ExifTool::Fixup::AddFixup", add_fixup);
    it.define_native("Image::ExifTool::Fixup::ApplyFixup", apply_fixup);
    it.define_native("Image::ExifTool::Fixup::IsEmpty", is_empty);
    it.define_native("Image::ExifTool::Fixup::HasMarker", has_marker);
    it.define_native("Image::ExifTool::Fixup::SetMarkerPointers", set_marker_pointers);
    it.define_native("Image::ExifTool::Fixup::GetMarkerPointers", get_marker_pointers);
    it.define_native("Image::ExifTool::Fixup::Dump", dump);
}
