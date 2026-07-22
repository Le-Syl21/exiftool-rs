//! MXF (Material Exchange Format) format reader.
//!
//! Ported from Perl ExifTool 13.59 `MXF.pm`. An MXF file is a flat sequence of
//! KLV triplets, but the metadata it carries describes an OBJECT GRAPH: every
//! "local set" carries an InstanceUID and links to other sets through strong
//! references. ExifTool parses the file linearly, then walks that graph from
//! each Preface to
//!
//!   * give every object the family-1 group of the track it belongs to
//!     (`Track1`, `Track2`, … — `SetGroups`, MXF.pm:2731-2778),
//!   * scale every duration by the EditRate of that track
//!     (`ConvertDurations`, MXF.pm:2783-2801),
//!   * drop the copies of an object that other partitions repeat, keeping the
//!     last one in file order (`ProcessMXF`, MXF.pm:2939-2963).
//!
//! This module reproduces that: it retains the parsed objects, walks them, and
//! only then emits.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;
use std::collections::{HashMap, HashSet};

/// The family-1 group every MXF tag starts in: `$$et{SET_GROUP1} = 'MXF'`
/// (MXF.pm:2836). `SetGroups` overwrites it for objects that belong to a track.
const DEFAULT_GROUP1: &str = "MXF";

/// `InstanceUID` (MXF.pm:182) — the identity of a local set. `Unknown => 1`, so
/// it is never emitted; it only keys the object table.
const INSTANCE_UID: &str = "060e2b34010101010101150200000000";

/// Local sets ExifTool descends into: the `%localSet` entries of
/// `MXF::Main` (MXF.pm:2334-2409). The name is ExifTool's `DIR_NAME`, which
/// `SetGroups` tests against `Preface`, `SourcePackage` and `TimecodeComponent`.
///
/// The sets MXF.pm describes as `Unknown => 1` instead are in
/// [`UNKNOWN_SETS`]: ExifTool skips their value entirely.
///
/// Sorted by UL for `binary_search_by_key`.
#[rustfmt::skip]
const LOCAL_SETS: &[(&str, &str)] = &[
    ("060e2b34025301010d01010101010200", "StructuralComponent"),
    ("060e2b34025301010d01010101010f00", "SequenceSet"),
    ("060e2b34025301010d01010101011400", "TimecodeComponent"),
    ("060e2b34025301010d01010101011800", "ContentStorageSet"),
    ("060e2b34025301010d01010101012300", "EssenceContainerDataSet"),
    ("060e2b34025301010d01010101012500", "FileDescriptor"),
    ("060e2b34025301010d01010101012700", "GenericPictureEssenceDescriptor"),
    ("060e2b34025301010d01010101012800", "CDCIEssenceDescriptor"),
    ("060e2b34025301010d01010101012900", "RGBAEssenceDescriptor"),
    ("060e2b34025301010d01010101012f00", "Preface"),
    ("060e2b34025301010d01010101013000", "Identification"),
    ("060e2b34025301010d01010101013200", "NetworkLocator"),
    ("060e2b34025301010d01010101013300", "TextLocator"),
    ("060e2b34025301010d01010101013400", "GenericPackage"),
    ("060e2b34025301010d01010101013600", "MaterialPackage"),
    ("060e2b34025301010d01010101013700", "SourcePackage"),
    ("060e2b34025301010d01010101013800", "GenericTrack"),
    ("060e2b34025301010d01010101013900", "EventTrack"),
    ("060e2b34025301010d01010101013a00", "StaticTrack"),
    ("060e2b34025301010d01010101013b00", "Track"),
    ("060e2b34025301010d01010101014100", "DMSegment"),
    ("060e2b34025301010d01010101014200", "GenericSoundEssenceDescriptor"),
    ("060e2b34025301010d01010101014300", "GenericDataEssenceDescriptor"),
    ("060e2b34025301010d01010101014400", "MultipleDescriptor"),
    ("060e2b34025301010d01010101014500", "DMSourceClip"),
    ("060e2b34025301010d01010101014700", "AES3PCMDescriptor"),
    ("060e2b34025301010d01010101014800", "WaveAudioDescriptor"),
    ("060e2b34025301010d01010101015100", "MPEG2VideoDescriptor"),
    ("060e2b34025301010d01010101015a00", "JPEG2000PictureSubDescriptor"),
    ("060e2b34025301010d01010101015b00", "VBIDataDescriptor"),
    ("060e2b34025301010d01040000000000", "DMSet"),
    ("060e2b34025301010d01040100000000", "DMFramework"),
    ("060e2b34025301010d01040101010100", "ProductionFramework"),
    ("060e2b34025301010d01040101010200", "ClipFramework"),
    ("060e2b34025301010d01040101010300", "SceneFramework"),
    ("060e2b34025301010d01040101100100", "Titles"),
    ("060e2b34025301010d01040101110100", "Identification"),
    ("060e2b34025301010d01040101120100", "GroupRelationship"),
    ("060e2b34025301010d01040101130100", "Branding"),
    ("060e2b34025301010d01040101140100", "Event"),
    ("060e2b34025301010d01040101140200", "Publication"),
    ("060e2b34025301010d01040101150100", "Award"),
    ("060e2b34025301010d01040101160100", "CaptionDescription"),
    ("060e2b34025301010d01040101170100", "Annotation"),
    ("060e2b34025301010d01040101170200", "SettingPeriod"),
    ("060e2b34025301010d01040101170300", "Scripting"),
    ("060e2b34025301010d01040101170400", "Classification"),
    ("060e2b34025301010d01040101170500", "Shot"),
    ("060e2b34025301010d01040101170600", "KeyPoint"),
    ("060e2b34025301010d01040101170800", "CueWords"),
    ("060e2b34025301010d01040101180100", "Participant"),
    ("060e2b34025301010d01040101190100", "ContactsList"),
    ("060e2b34025301010d010401011a0200", "Person"),
    ("060e2b34025301010d010401011a0300", "Organisation"),
    ("060e2b34025301010d010401011a0400", "Location"),
    ("060e2b34025301010d010401011b0100", "Address"),
    ("060e2b34025301010d010401011b0200", "Communications"),
    ("060e2b34025301010d010401011c0100", "Contract"),
    ("060e2b34025301010d010401011c0200", "Rights"),
    ("060e2b34025301010d010401011d0100", "PictureFormat"),
    ("060e2b34025301010d010401011e0100", "DeviceParameters"),
    ("060e2b34025301010d010401011f0100", "NameValue"),
    ("060e2b34025301010d01040101200100", "Processing"),
    ("060e2b34025301010d01040101200200", "Projects"),
    ("060e2b34025301010d01040102010000", "CryptographicFramework"),
    ("060e2b34025301010d01040102020000", "CryptographicContext"),
];

/// Name ExifTool invents for an undescribed local set under the
/// `060e2b34.0253.0101.0d` branch (MXF.pm:2871-2879): it adds it to the table
/// with `%localSet`, so its contents ARE parsed.
const USER_ORGANIZATION_SET: &str = "UserOrganizationPublicUse";

/// Prefix of the ULs that trigger [`USER_ORGANIZATION_SET`] (MXF.pm:2872).
const USER_ORGANIZATION_PREFIX: &str = "060e2b34025301010d";

/// Sets under that same branch that `MXF::Main` DOES describe, as
/// `Unknown => 1`. ExifTool only invents a `UserOrganizationPublicUse` entry
/// when the key has no tagInfo at all (`if (not $tagInfo and ...`,
/// MXF.pm:2871), so these keep their own definition and their value is skipped.
#[rustfmt::skip]
const UNKNOWN_SETS: &[&str] = &[
    // SourceClip: "actually a local set, but it isn't decoded because it has a
    // Duration tag which gets confused with the other Duration tags" (MXF.pm:2336-2339)
    "060e2b34025301010d01010101011100",
    "060e2b34025301010d01020101100000", // V10IndexTableSegment (MXF.pm:2369)
    "060e2b34025301010d01020101100100", // IndexTableSegment (MXF.pm:2370)
];

/// Tags whose value links to other objects: every `Type => 'StrongReference'`,
/// `'StrongReferenceArray'` or `'StrongReferenceBatch'` entry of `MXF::Main`.
/// The flag is true for the Array/Batch forms, which are counted collections
/// rather than a single 16-byte UID (MXF.pm:2525-2537).
///
/// All of them are `Unknown => 1`, so they are followed but never emitted.
/// Sorted by UL for `binary_search_by_key`.
#[rustfmt::skip]
const STRONG_REFS: &[(&str, bool)] = &[
    ("060e2b34010101020301021003000000", true),  // PackageKLVData
    ("060e2b34010101020301021004000000", true),  // ComponentKLVData
    ("060e2b3401010102030201020c000000", true),  // PackageUserComments
    ("060e2b34010101020520090d00000000", true),  // Plug-InLocatorSet
    ("060e2b34010101020601010402010000", false), // ContentStorage
    ("060e2b34010101020601010402020000", false), // Dictionary
    ("060e2b34010101020601010402030000", false), // EssenceDescription
    ("060e2b34010101020601010402040000", false), // Sequence
    ("060e2b34010101020601010402050000", false), // TransitionEffect
    ("060e2b34010101020601010402060000", false), // EffectRendering
    ("060e2b34010101020601010402070000", false), // InputSegment
    ("060e2b34010101020601010402080000", false), // StillFrame
    ("060e2b34010101020601010402090000", false), // Selected
    ("060e2b340101010206010104020a0000", false), // Annotation
    ("060e2b340101010206010104020b0000", false), // ManufacturerInformationObject
    ("060e2b34010101020601010405010000", true),  // Packages
    ("060e2b34010101020601010405020000", true),  // EssenceData
    ("060e2b34010101020601010405030000", true),  // OperationDefinitions
    ("060e2b34010101020601010405040000", true),  // ParameterDefinitions
    ("060e2b34010101020601010405050000", true),  // DataDefinitions
    ("060e2b34010101020601010405060000", true),  // Plug-InDefinitions
    ("060e2b34010101020601010405070000", true),  // CodecDefinitions
    ("060e2b34010101020601010405080000", true),  // ContainerDefinitions
    ("060e2b34010101020601010405090000", true),  // InterpolationDefinitions
    ("060e2b34010101020601010406010000", true),  // AvailableRepresentations
    ("060e2b34010101020601010406020000", true),  // InputSegments
    ("060e2b34010101020601010406030000", true),  // EssenceLocators
    ("060e2b34010101020601010406040000", true),  // IdentificationList
    ("060e2b34010101020601010406050000", true),  // Tracks
    ("060e2b34010101020601010406060000", true),  // ControlPointList
    ("060e2b34010101020601010406070000", true),  // PackageTracks
    ("060e2b34010101020601010406080000", true),  // Alternates
    ("060e2b34010101020601010406090000", true),  // ComponentsInSequence
    ("060e2b340101010206010104060a0000", true),  // Parameters
    ("060e2b34010101020601010702000000", true),  // Properties
    ("060e2b34010101020601010707000000", true),  // ClassDefinitions
    ("060e2b34010101020601010708000000", true),  // TypeDefinitions
    ("060e2b340101010406010104060b0000", true),  // FileDescriptors
    ("060e2b340101010506010104020c0000", false), // DescriptiveMetadataFramework
    ("060e2b34010101050601010402400500", false), // GroupSet
    ("060e2b34010101050601010402401c00", false), // BankDetailsSet
    ("060e2b34010101050601010402401d00", false), // ImageFormatSet
    ("060e2b34010101050601010402402000", false), // ProcessingSet
    ("060e2b34010101050601010402402100", false), // ProjectSet
    ("060e2b34010101050601010402402200", false), // ContactsListSet
    ("060e2b34010101050601010402402301", false), // AnnotationCueWordsSet
    ("060e2b34010101050601010402402302", false), // ShotCueWordsSet
    ("060e2b34010101050601010405400400", true),  // TitlesSets
    ("060e2b34010101050601010405400500", true),  // GroupSets
    ("060e2b34010101050601010405400600", true),  // IdentificationSets
    ("060e2b34010101050601010405400700", true),  // EpisodicItemSets
    ("060e2b34010101050601010405400800", true),  // BrandingSets
    ("060e2b34010101050601010405400900", true),  // EventSets
    ("060e2b34010101050601010405400a00", true),  // PublicationSets
    ("060e2b34010101050601010405400b00", true),  // AwardSets
    ("060e2b34010101050601010405400c00", true),  // CaptionDescriptionSets
    ("060e2b34010101050601010405400d00", true),  // AnnotationSets
    ("060e2b34010101050601010405400e01", true),  // ProductionSettingPeriodSets
    ("060e2b34010101050601010405400e02", true),  // SceneSettingPeriodSets
    ("060e2b34010101050601010405400f00", true),  // ScriptingSets
    ("060e2b34010101050601010405401000", true),  // ClassificationSets
    ("060e2b34010101050601010405401101", true),  // SceneShotSets
    ("060e2b34010101050601010405401102", true),  // ClipShotSets
    ("060e2b34010101050601010405401200", true),  // KeyPointSets
    ("060e2b34010101050601010405401300", true),  // ShotParticipantRoleSets
    ("060e2b34010101050601010405401400", true),  // ShotPersonSets
    ("060e2b34010101050601010405401500", true),  // OrganizationSets
    ("060e2b34010101050601010405401600", true),  // ShotLocationSets
    ("060e2b34010101050601010405401700", true),  // AddressSets
    ("060e2b34010101050601010405401800", true),  // CommunicationSets
    ("060e2b34010101050601010405401900", true),  // ContractSets
    ("060e2b34010101050601010405401a00", true),  // RightsSets
    ("060e2b34010101050601010405401b00", true),  // PaymentsSets
    ("060e2b34010101050601010405401e00", true),  // DeviceParametersSets
    ("060e2b34010101050601010405401f01", true),  // ClassificationNameValueSets
    ("060e2b34010101050601010405401f02", true),  // ContactNameValueSets
    ("060e2b34010101050601010405401f03", true),  // DeviceParameterNameValueSets
    ("060e2b340101010506010104060c0000", true),  // MetadataServerLocators
    ("060e2b340101010506010104060d0000", true),  // RelatedMaterialLocators
    ("060e2b34010101070301021007000000", true),  // PackageAttributes
    ("060e2b34010101070301021008000000", true),  // ComponentAttributes
    ("060e2b34010101070302010216000000", true),  // ComponentUserComments
    ("060e2b340101010706010104050a0000", true),  // KLVDataDefinitions
    ("060e2b340101010706010104050b0000", true),  // TaggedValueDefinitions
    ("060e2b34010101070601010405401f04", true),  // AddressNameValueSets
    ("060e2b34010101080601010405400d01", true),  // EventAnnotationSets
    ("060e2b340101010806010104060e0000", true),  // ScriptingLocators
    ("060e2b340101010806010104060f0000", true),  // UnknownBWFChunks
    ("060e2b340101010906010104020d0000", false), // CryptographicContextObject
    ("060e2b34010101090601010406100000", true),  // Sub-descriptors
    ("060e2b340101010a0601010716000000", false), // RootMetaDictionary
    ("060e2b340101010a0601010717000000", false), // RootPreface
    ("060e2b340101010c06010104020e0000", false), // ApplicationPlug-InBatch
    ("060e2b340101010c06010104020f0000", false), // PackageMarker
    ("060e2b340101010c0601010402100000", false), // PackageTimelineMarkerRef
    ("060e2b340101010c0601010402110000", false), // RegisterAdministrationObject
    ("060e2b340101010c0601010402120000", false), // RegisterEntryAdministrationObject
    ("060e2b340101010c0601010406110000", true),  // RegisterEntryArray
    ("060e2b340101010c0601010406120000", true),  // RegisterAdministrationArray
    ("060e2b340101010c0601010406130000", true),  // ApplicationInformationArray
    ("060e2b340101010c0601010406140000", true),  // RegisterChildEntryArray
];

/// `%componentDataDef` (MXF.pm:100-110).
#[rustfmt::skip]
const COMPONENT_DATA_DEF: &[(&str, &str)] = &[
    ("060e2b34.0401.0101.01030201.01000000", "SMPTE 12M Timecode Track"),
    ("060e2b34.0401.0101.01030201.02000000", "SMPTE 12M Timecode Track with active user bits"),
    ("060e2b34.0401.0101.01030201.03000000", "SMPTE 309M Timecode Track"),
    ("060e2b34.0401.0101.01030201.10000000", "Descriptive Metadata Track"),
    ("060e2b34.0401.0101.01030202.01000000", "Picture Essence Track"),
    ("060e2b34.0401.0101.01030202.02000000", "Sound Essence Track"),
    ("060e2b34.0401.0101.01030202.03000000", "Data Essence Track"),
];

/// How to turn a local-set value into a tag value, one variant per `Type` /
/// `Format` combination of `MXF::Main` that this reader covers.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// `%timestamp` (MXF.pm:74-78).
    Timestamp,
    /// `Type => 'UTF-16'` (MXF.pm:2483).
    Utf16,
    /// `Type => 'VersionType'` (MXF.pm:2491).
    VersionType,
    /// `Type => 'ProductVersion'` (MXF.pm:2485).
    ProductVersion,
    /// `Type => 'Boolean'` (MXF.pm:2508).
    Boolean,
    /// `Format => 'int16u'`.
    Int16u,
    /// `Format => 'int32u'`.
    Int32u,
    /// `Format => 'rational64s'`.
    Rational,
    /// `Format => 'int64s'` plus `%duration` (MXF.pm:96-100).
    Int64sDuration,
    /// `Type => 'Length'` plus `%duration`: `Get64u` (MXF.pm:2506).
    LengthDuration,
    /// `Type => 'WeakReference'` with the `%componentDataDef` PrintConv.
    ComponentDataDef,
}

/// The `MXF::Main` entries this reader decodes, `(UL, name, kind)`.
/// Everything else in the local set is skipped, which matches ExifTool for the
/// `Unknown => 1` entries and is a coverage gap for the rest.
#[rustfmt::skip]
const TAGS: &[(&str, &str, Kind)] = &[
    // MXF.pm:522
    ("060e2b34010101010404010105000000", "DropFrame", Kind::Boolean),
    // MXF.pm:538-539
    ("060e2b34010101010406010100000000", "SampleRate", Kind::Rational),
    ("060e2b34010101010406010200000000", "EssenceLength", Kind::LengthDuration),
    // MXF.pm:974-982
    ("060e2b34010101020104010300000000", "TrackNumber", Kind::Int32u),
    ("060e2b34010101020107010100000000", "TrackID", Kind::Int32u),
    ("060e2b34010101020107010201000000", "TrackName", Kind::Utf16),
    // MXF.pm:1003
    ("060e2b34010101020301020105000000", "SDKVersion", Kind::VersionType),
    // MXF.pm:1077-1079
    ("060e2b34010101020404010102060000", "RoundedTimecodeTimebase", Kind::Int16u),
    ("060e2b34010101020407010000000000", "ComponentDataDefinition", Kind::ComponentDataDef),
    // MXF.pm:1128-1139
    ("060e2b34010101020520070102010000", "ApplicationSupplierName", Kind::Utf16),
    ("060e2b34010101020520070103010000", "ApplicationName", Kind::Utf16),
    ("060e2b34010101020520070105010000", "ApplicationVersionString", Kind::Utf16),
    ("060e2b34010101020520070106010000", "ApplicationPlatform", Kind::Utf16),
    ("060e2b3401010102052007010a000000", "ToolkitVersion", Kind::ProductVersion),
    // MXF.pm:1160
    ("060e2b34010101020530040500000000", "EditRate", Kind::Rational),
    // MXF.pm:1262-1264
    ("060e2b34010101020702010301030000", "Origin", Kind::Int64sDuration),
    ("060e2b34010101020702010301050000", "StartTimecode", Kind::Int64sDuration),
    // MXF.pm:1269-1273
    ("060e2b34010101020702011001030000", "CreateDate", Kind::Timestamp),
    ("060e2b34010101020702011002030000", "ModifyDate", Kind::Timestamp),
    ("060e2b34010101020702011002040000", "ContainerLastModifyDate", Kind::Timestamp),
    ("060e2b34010101020702011002050000", "PackageLastModifyDate", Kind::Timestamp),
    ("060e2b34010101020702020101030000", "Duration", Kind::LengthDuration),
    // MXF.pm:1538
    ("060e2b34010101040103040400000000", "EssenceStreamID", Kind::Int32u),
    // MXF.pm:1615-1616
    ("060e2b34010101040402030104000000", "LockedIndicator", Kind::Boolean),
    ("060e2b34010101040402030304000000", "BitsPerAudioSample", Kind::Int32u),
    // MXF.pm:1795-1804
    ("060e2b34010101050402010104000000", "ChannelCount", Kind::Int32u),
    ("060e2b34010101050402030101010000", "AudioSampleRate", Kind::Rational),
    ("060e2b34010101050402030201000000", "BlockAlign", Kind::Int16u),
    ("060e2b34010101050402030305000000", "AverageBytesPerSecond", Kind::Int32u),
    // MXF.pm:1863
    ("060e2b34010101050601010305000000", "LinkedTrackID", Kind::Int32u),
];

/// One tag as parsed, with what the object walk still needs to know about it.
struct Emitted {
    tag: Tag,
    /// InstanceUID of the local set it came from, `None` for a set that
    /// declared none (its tags are then never deduplicated, matching ExifTool:
    /// `$$_{UID} = $instance foreach @groups` only runs `if ($instance)`).
    uid: Option<[u8; 16]>,
    /// Raw edit-unit count of an `IsDuration` tag, before `ConvertDurations`.
    duration: Option<f64>,
}

/// A local set, keyed by its InstanceUID. Copies of the same object in
/// different partitions merge into one, exactly as ExifTool merges them
/// (MXF.pm:2688-2697).
struct Obj {
    /// ExifTool's `DIR_NAME` for the set.
    name: &'static str,
    strong_refs: Vec<[u8; 16]>,
    /// Indices into [`Parser::emitted`].
    tags: Vec<usize>,
    track_id: Option<u32>,
    edit_rate: Option<f64>,
    /// `$$objInfo{DidGroups}` — an object is walked once (MXF.pm:2735).
    did_groups: bool,
}

#[derive(Default)]
struct Parser {
    emitted: Vec<Emitted>,
    objects: Vec<Obj>,
    by_uid: HashMap<[u8; 16], usize>,
    /// `$$mxfInfo{Group1}`: TrackID -> group name, numbered in the order the
    /// track IDs are met while parsing (MXF.pm:2681-2683).
    group1: HashMap<u32, String>,
    num_tracks: u32,
    /// InstanceUIDs of every Preface, the roots of the walk (MXF.pm:2718).
    prefaces: Vec<[u8; 16]>,
    /// `$$mxfInfo{EditRate}`: family-1 group -> edit rate (MXF.pm:2747).
    edit_rates: HashMap<String, f64>,
    /// `$$mxfInfo{BestDuration}`, keyed `"Source"` / `"Other"` (MXF.pm:2751).
    best_duration: HashMap<&'static str, [u8; 16]>,
    /// `$$mxfInfo{InSource}` (MXF.pm:2756).
    in_source: bool,
}

pub fn read_mxf(data: &[u8], extract_embedded: u8) -> Result<Vec<Tag>> {
    // Look for MXF KLV start marker: 06 0e 2b 34
    let magic = b"\x06\x0e\x2b\x34";
    let start = data
        .windows(4)
        .position(|w| w == magic.as_ref())
        .ok_or_else(|| Error::InvalidData("not an MXF file".into()))?;

    let data = &data[start..];
    let mut p = Parser::default();
    let mut registry: HashMap<[u8; 2], [u8; 16]> = HashMap::new();

    let mut pos = 0;
    while pos + 17 <= data.len() {
        if &data[pos..pos + 4] != b"\x06\x0e\x2b\x34" {
            pos += 1;
            continue;
        }
        let key = &data[pos..pos + 16];

        // Parse BER length at pos+16
        let len_byte = data[pos + 16];
        let (val_len, ber_size) = if len_byte < 0x80 {
            (len_byte as usize, 1usize)
        } else {
            let n = (len_byte & 0x7f) as usize;
            if pos + 17 + n > data.len() {
                break;
            }
            let mut l = 0usize;
            for i in 0..n {
                l = (l << 8) | (data[pos + 17 + i] as usize);
            }
            (l, 1 + n)
        };
        let val_start = pos + 16 + ber_size;
        if val_start + val_len > data.len() {
            break;
        }
        let val = &data[val_start..val_start + val_len];

        // Header partition (%header, MXF.pm:2317-2320): MXFVersion at offset 0
        // of MXF::Header (MXF.pm:2422-2426). The other two entries of that table
        // are RawConv'd away or Unknown.
        if key[4] == 0x02 && key[5] == 0x05 && key[12] == 0x01 && key[13] == 0x02 {
            if val.len() >= 4 && !p.emitted.iter().any(|e| e.tag.name == "MXFVersion") {
                let major = u16::from_be_bytes([val[0], val[1]]);
                let minor = u16::from_be_bytes([val[2], val[3]]);
                p.emitted.push(Emitted {
                    tag: mktag(
                        DEFAULT_GROUP1,
                        "MXFVersion",
                        "MXF Version",
                        Value::String(format!("{major}.{minor}")),
                    ),
                    uid: None,
                    duration: None,
                });
            }
        }
        // Primer Pack (MXF.pm:2329, ProcessPrimer at MXF.pm:2568-2596)
        else if key[4] == 0x02 && key[5] == 0x05 && key[12] == 0x01 && key[13] == 0x05 {
            read_primer(val, &mut registry);
        }
        // Local set
        else if let Some(set_name) = local_set_name(&hex32(key)) {
            parse_local_set(val, set_name, &registry, &mut p);
        }

        pos = val_start + val_len;
    }

    // Walk the whole object tree to fix family-1 group names (MXF.pm:2939-2941)
    for root in p.prefaces.clone() {
        set_groups(&mut p, root);
    }
    convert_durations(&mut p);
    let mut tags = finish(p);

    // Without -ee the CLI leaves Duplicates off (`exiftool` line 1031 turns it
    // ON for -ee), so only one tag per name survives; ExifTool keeps the last
    // one found at equal priority.
    if extract_embedded == 0 {
        let mut last: HashMap<&str, usize> = HashMap::new();
        for (i, t) in tags.iter().enumerate() {
            last.insert(t.name.as_str(), i);
        }
        let keep: Vec<bool> = tags
            .iter()
            .enumerate()
            .map(|(i, t)| last.get(t.name.as_str()) == Some(&i))
            .collect();
        let mut it = keep.into_iter();
        tags.retain(|_| it.next().unwrap_or(true));
    }

    Ok(tags)
}

/// `ProcessPrimer` (MXF.pm:2568-2596): local tag ID -> global UL.
fn read_primer(val: &[u8], registry: &mut HashMap<[u8; 2], [u8; 16]>) {
    if val.len() < 8 {
        return;
    }
    let count = u32::from_be_bytes([val[0], val[1], val[2], val[3]]) as usize;
    let item_size = u32::from_be_bytes([val[4], val[5], val[6], val[7]]) as usize;
    if item_size < 18 {
        return;
    }
    for i in 0..count {
        let off = 8 + i * item_size;
        if off + 18 > val.len() {
            break;
        }
        let mut ltag = [0u8; 2];
        ltag.copy_from_slice(&val[off..off + 2]);
        registry.insert(ltag, uid16(&val[off + 2..off + 18]));
    }
}

/// The set name ExifTool descends into this KLV key with, or `None` when it
/// skips the value (MXF.pm:2869-2879).
fn local_set_name(ul: &str) -> Option<&'static str> {
    if let Ok(i) = LOCAL_SETS.binary_search_by_key(&ul, |e| e.0) {
        return Some(LOCAL_SETS[i].1);
    }
    // Unknown sets under the "user organization, public use" branch are added to
    // the table on the fly, still as local sets (MXF.pm:2871-2879).
    if ul.starts_with(USER_ORGANIZATION_PREFIX) && !UNKNOWN_SETS.contains(&ul) {
        return Some(USER_ORGANIZATION_SET);
    }
    None
}

/// `ProcessLocalSet` (MXF.pm:2598-2721).
fn parse_local_set(
    data: &[u8],
    set_name: &'static str,
    registry: &HashMap<[u8; 2], [u8; 16]>,
    p: &mut Parser,
) {
    let mut strong_refs: Vec<[u8; 16]> = Vec::new();
    let mut indices: Vec<usize> = Vec::new();
    let mut instance: Option<[u8; 16]> = None;
    let mut edit_rate: Option<f64> = None;
    let mut track_id: Option<u32> = None;

    let mut pos = 0;
    while pos + 4 <= data.len() {
        let ltag = [data[pos], data[pos + 1]];
        let llen = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + llen > data.len() {
            break;
        }
        let val = &data[pos..pos + llen];
        pos += llen;

        let Some(ul) = registry.get(&ltag) else {
            continue;
        };
        let ul = hex32(ul);

        if ul == INSTANCE_UID {
            if val.len() == 16 {
                instance = Some(uid16(val));
            }
            continue;
        }
        if let Ok(i) = STRONG_REFS.binary_search_by_key(&ul.as_str(), |e| e.0) {
            strong_refs.extend(read_strong_refs(val, STRONG_REFS[i].1));
            continue;
        }
        let Ok(i) = TAGS.binary_search_by_key(&ul.as_str(), |e| e.0) else {
            continue;
        };
        let (_, name, kind) = TAGS[i];
        let Some((text, duration)) = decode(kind, val) else {
            continue;
        };

        // MXF.pm:2677-2684: the EditRate and the track ID of this object are
        // taken from the tags themselves, the track ID from ANY tag whose name
        // ends in TrackID (which is how a WaveAudioDescriptor joins its track
        // through LinkedTrackID).
        if name == "EditRate" {
            edit_rate = rational(val);
        } else if name.ends_with("TrackID") {
            if let Some(id) = int32u(val) {
                track_id = Some(id);
                if !p.group1.contains_key(&id) {
                    p.num_tracks += 1;
                    p.group1.insert(id, format!("Track{}", p.num_tracks));
                }
            }
        }

        indices.push(p.emitted.len());
        p.emitted.push(Emitted {
            tag: mktag(DEFAULT_GROUP1, name, name, Value::String(text)),
            uid: None,
            duration,
        });
    }

    // Save the object now that the instance UID is known (MXF.pm:2687-2720).
    let Some(uid) = instance else { return };
    let idx = match p.by_uid.get(&uid) {
        Some(&i) => i,
        None => {
            p.objects.push(Obj {
                name: set_name,
                strong_refs: Vec::new(),
                tags: Vec::new(),
                track_id: None,
                edit_rate: None,
                did_groups: false,
            });
            let i = p.objects.len() - 1;
            p.by_uid.insert(uid, i);
            i
        }
    };
    let obj = &mut p.objects[idx];
    obj.name = set_name;
    obj.strong_refs.extend(strong_refs);
    obj.tags.extend(indices.iter().copied());
    if track_id.is_some() {
        obj.track_id = track_id;
    }
    // `$$objInfo{EditRate} = $editRate if $editRate` — a zero rate is falsy in
    // Perl and would divide by zero here too.
    if let Some(r) = edit_rate {
        if r != 0.0 {
            obj.edit_rate = Some(r);
        }
    }
    for i in indices {
        p.emitted[i].uid = Some(uid);
    }
    if set_name == "Preface" {
        p.prefaces.push(uid);
    }
}

/// `SetGroups` (MXF.pm:2731-2778), as an explicit-stack depth-first walk so a
/// long reference chain cannot overflow the stack.
fn set_groups(p: &mut Parser, root: [u8; 16]) {
    /// A pending step of the walk.
    enum Step {
        /// Visit this object, inheriting this track ID from its parent.
        Visit([u8; 16], Option<u32>),
        /// `delete $$mxfInfo{InSource}` once a SourcePackage subtree is done.
        LeaveSource,
    }
    let mut stack = vec![Step::Visit(root, None)];
    while let Some(step) = stack.pop() {
        let (uid, inherited) = match step {
            Step::LeaveSource => {
                p.in_source = false;
                continue;
            }
            Step::Visit(uid, inherited) => (uid, inherited),
        };
        let Some(&i) = p.by_uid.get(&uid) else {
            continue;
        };
        if p.objects[i].did_groups {
            continue;
        }
        p.objects[i].did_groups = true;

        // The track ID flows down the tree and is overridden by an object that
        // declares its own (MXF.pm:2736).
        let track_id = p.objects[i].track_id.or(inherited);
        let mut g1 = None;
        if let Some(id) = track_id {
            p.objects[i].track_id = Some(id);
            g1 = p.group1.get(&id).cloned();
            if let (Some(rate), Some(g)) = (p.objects[i].edit_rate, g1.as_ref()) {
                p.edit_rates.insert(g.clone(), rate);
            }
            if p.objects[i].name == "TimecodeComponent" {
                let in_what = if p.in_source { "Source" } else { "Other" };
                p.best_duration.insert(in_what, uid);
            }
        }
        // The SourcePackage holds the preferred TimecodeComponent (MXF.pm:2756).
        let set_source = p.objects[i].name == "SourcePackage";
        if set_source {
            p.in_source = true;
        }
        if let Some(g) = g1 {
            for ti in p.objects[i].tags.clone() {
                p.emitted[ti].tag.group.family1 = g.clone();
            }
        }
        // Push the post-action first so it runs after the whole subtree, and the
        // children reversed so they are visited in reference order.
        if set_source {
            stack.push(Step::LeaveSource);
        }
        for r in p.objects[i].strong_refs.clone().into_iter().rev() {
            stack.push(Step::Visit(r, track_id));
        }
    }
}

/// `ConvertDurations` (MXF.pm:2783-2801): a duration is stored in edit units,
/// so divide it by the EditRate of the group it ended up in.
fn convert_durations(p: &mut Parser) {
    for e in &mut p.emitted {
        let Some(d) = e.duration else { continue };
        let d = match p.edit_rates.get(e.tag.group.family1.as_str()) {
            Some(rate) if *rate != 0.0 => d / rate,
            _ => d,
        };
        e.duration = Some(d);
        let text = convert_duration(d);
        e.tag.raw_value = Value::String(text.clone());
        e.tag.print_value = text;
    }
}

/// The last block of `ProcessMXF` (MXF.pm:2943-2963): keep only the tags of the
/// most recent copy of each object, and emit the best Duration.
fn finish(p: Parser) -> Vec<Tag> {
    let best_uid = p
        .best_duration
        .get("Source")
        .or_else(|| p.best_duration.get("Other"))
        .copied();
    let mut best_value = None;

    let mut seen: HashSet<(&str, [u8; 16])> = HashSet::new();
    let mut keep = vec![true; p.emitted.len()];
    for (i, e) in p.emitted.iter().enumerate().rev() {
        let Some(uid) = e.uid else { continue };
        if !seen.insert((e.tag.name.as_str(), uid)) {
            keep[i] = false;
        } else if Some(uid) == best_uid && e.tag.name == "Duration" {
            best_value = e.duration;
        }
    }

    let mut tags: Vec<Tag> = p
        .emitted
        .iter()
        .zip(&keep)
        .filter(|(_, &k)| k)
        .map(|(e, _)| e.tag.clone())
        .collect();

    // The preferred TimecodeComponent duration is re-extracted as a plain MXF
    // Duration -- UL 060e2b34.0101.0102.07020201.01030000 -- which puts it back
    // through that tag's RawConv (MXF.pm:2960). It carries no G1 override, so it
    // lands in the file-level MXF group.
    if let Some(v) = best_value {
        if v.abs() <= 1e18 {
            tags.push(mktag(
                DEFAULT_GROUP1,
                "Duration",
                "Duration",
                Value::String(convert_duration(v)),
            ));
        }
    }
    tags
}

/// `ReadMXFValue` for the strong-reference types (MXF.pm:2525-2541). The
/// Array/Batch forms are a count/size header followed by the entries; the plain
/// form is a single 16-byte UID.
fn read_strong_refs(val: &[u8], collection: bool) -> Vec<[u8; 16]> {
    if collection && val.len() > 16 {
        let count = u32::from_be_bytes([val[0], val[1], val[2], val[3]]) as usize;
        let size = u32::from_be_bytes([val[4], val[5], val[6], val[7]]) as usize;
        let mut out = Vec::new();
        for i in 0..count {
            let off = 8 + i * size;
            if size < 16 || off + size > val.len() {
                break;
            }
            out.push(uid16(&val[off..off + 16]));
        }
        return out;
    }
    if val.len() == 16 {
        return vec![uid16(val)];
    }
    Vec::new()
}

/// Decode one local-set value. Returns the printed value and, for an
/// `IsDuration` tag, the raw edit-unit count still to be scaled. `None` means
/// ExifTool extracts no tag here (a short value, or `%duration`'s
/// `RawConv => '$val > 1e18 ? undef : $val'`, MXF.pm:98).
fn decode(kind: Kind, val: &[u8]) -> Option<(String, Option<f64>)> {
    let text = match kind {
        Kind::Timestamp => decode_timestamp(val),
        Kind::Utf16 => decode_utf16(val),
        Kind::VersionType => val
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join("."),
        Kind::ProductVersion => decode_product_version(val),
        // `$val eq "\0" ? 'False' : 'True'` (MXF.pm:2509).
        Kind::Boolean => if val == b"\0" { "False" } else { "True" }.to_string(),
        Kind::Int16u => {
            if val.len() < 2 {
                return None;
            }
            u16::from_be_bytes([val[0], val[1]]).to_string()
        }
        Kind::Int32u => int32u(val)?.to_string(),
        Kind::Rational => format_number(rational(val)?),
        Kind::ComponentDataDef => decode_component_data_def(val),
        Kind::Int64sDuration | Kind::LengthDuration => {
            if val.len() < 8 {
                return None;
            }
            let raw = <[u8; 8]>::try_from(&val[..8]).ok()?;
            // `Type => 'Length'` reads unsigned (Get64u, MXF.pm:2506);
            // `Format => 'int64s'` reads signed.
            let n = if kind == Kind::LengthDuration {
                u64::from_be_bytes(raw) as f64
            } else {
                i64::from_be_bytes(raw) as f64
            };
            if n > 1e18 {
                return None;
            }
            return Some((convert_duration(n), Some(n)));
        }
    };
    Some((text, None))
}

fn int32u(val: &[u8]) -> Option<u32> {
    if val.len() < 4 {
        return None;
    }
    Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]))
}

/// `rational64s`: two int32s, read as their quotient (ExifTool `GetRational64s`).
fn rational(val: &[u8]) -> Option<f64> {
    if val.len() < 8 {
        return None;
    }
    let num = i32::from_be_bytes([val[0], val[1], val[2], val[3]]) as f64;
    let den = i32::from_be_bytes([val[4], val[5], val[6], val[7]]) as f64;
    if den == 0.0 {
        return None;
    }
    Some(num / den)
}

/// Perl stringifies a double with `%.15g`; reproduce that for the numeric
/// values MXF stores (rates and counts), falling back to Rust's shortest form
/// outside the range where `%g` stays in fixed notation.
fn format_number(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    if !v.is_finite() {
        return format!("{v}");
    }
    let exp = v.abs().log10().floor() as i32;
    if !(-4..15).contains(&exp) {
        return format!("{v}");
    }
    let precision = (14 - exp).max(0) as usize;
    let s = format!("{v:.precision$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// `ConvertDuration` (ExifTool.pm:6877-6895).
fn convert_duration(time: f64) -> String {
    if time == 0.0 {
        return "0 s".to_string();
    }
    let (mut sign, mut time) = if time > 0.0 {
        (String::new(), time)
    } else {
        ("-".to_string(), -time)
    };
    if time < 30.0 {
        return format!("{sign}{time:.2} s");
    }
    time += 0.5; // to round off to nearest second
    let mut h = (time / 3600.0).trunc();
    time -= h * 3600.0;
    let m = (time / 60.0).trunc();
    time -= m * 60.0;
    if h > 24.0 {
        let d = (h / 24.0).trunc();
        h -= d * 24.0;
        sign = format!("{sign}{d} days ");
    }
    format!("{sign}{}:{:02}:{:02}", h as i64, m as i64, time as i64)
}

/// `Type => 'Timestamp'` (MXF.pm:2493-2505).
fn decode_timestamp(val: &[u8]) -> String {
    let fields: Vec<u32> = if val.len() >= 2 {
        std::iter::once(u16::from_be_bytes([val[0], val[1]]) as u32)
            .chain(val[2..].iter().map(|&b| b as u32))
            .collect()
    } else {
        Vec::new()
    };
    let max = [3000u32, 12, 31, 24, 59, 59, 249];
    let mut checked = 0;
    for f in &fields {
        if checked >= max.len() || *f > max[checked] {
            break;
        }
        checked += 1;
    }
    if checked < max.len() {
        let hex: String = val.iter().map(|b| format!("{b:02x}")).collect();
        return format!("Invalid (0x{hex})");
    }
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}",
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        fields[4],
        fields[5],
        fields[6] * 4,
    )
}

/// `Type => 'ProductVersion'` (MXF.pm:2485-2490).
fn decode_product_version(val: &[u8]) -> String {
    let mut a: Vec<u16> = val
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    while a.len() < 5 {
        a.push(0);
    }
    let release = match a[4] {
        0 => "unknown".to_string(),
        1 => "released".to_string(),
        2 => "debug".to_string(),
        3 => "patched".to_string(),
        4 => "beta".to_string(),
        5 => "private build".to_string(),
        n => format!("unknown {n}"),
    };
    format!("{}.{}.{}.{} {}", a[0], a[1], a[2], a[3], release)
}

/// `Type => 'UTF-16'` (MXF.pm:2483), big-endian (`SetByteOrder('MM')`).
fn decode_utf16(val: &[u8]) -> String {
    let chars: Vec<u16> = val
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&chars)
        .trim_end_matches('\0')
        .to_string()
}

/// `ComponentDataDefinition`, a `WeakReference` (MXF.pm:1079) run through
/// `%componentDataDef`. A 16-byte weak reference whose first byte has the high
/// bit clear is a UL and prints in dotted form (MXF.pm:2551-2554).
fn decode_component_data_def(val: &[u8]) -> String {
    if val.len() != 16 {
        return val.iter().map(|b| format!("{b:02x}")).collect();
    }
    let text = if val[0] & 0x80 == 0 {
        ul_dotted(val)
    } else {
        // A reversed GUID stored in a UL type: swap the high and low words and
        // print as a compact GUID (MXF.pm:2555-2559).
        let mut swapped = [0u8; 16];
        swapped[..8].copy_from_slice(&val[8..]);
        swapped[8..].copy_from_slice(&val[..8]);
        guid(&swapped)
    };
    COMPONENT_DATA_DEF
        .iter()
        .find(|(k, _)| *k == text)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or(text)
}

/// `UL` (MXF.pm:2452-2454): `H8.H4.H4.H8.H8`.
fn ul_dotted(b: &[u8]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}.{}.{}.{}.{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..24],
        &h[24..32]
    )
}

/// Compact GUID format, `H8-H4-H4-H4-H12` (MXF.pm:2560).
fn guid(b: &[u8]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn hex32(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn uid16(b: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&b[..16]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted() {
        assert!(LOCAL_SETS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(STRONG_REFS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(TAGS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn duration_formatting_matches_exiftool() {
        assert_eq!(convert_duration(0.0), "0 s");
        assert_eq!(convert_duration(1.5), "1.50 s");
        assert_eq!(convert_duration(-1.5), "-1.50 s");
        assert_eq!(convert_duration(90.0), "0:01:30");
        assert_eq!(convert_duration(90000.0), "1 days 1:00:00");
    }

    #[test]
    fn numbers_print_like_perl() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(7872.0), "7872");
        assert_eq!(format_number(30000.0 / 1001.0), "29.97002997003");
    }
}
