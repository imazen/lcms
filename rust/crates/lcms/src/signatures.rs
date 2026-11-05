//! Signature constants, generated from `reference/lcms2/include/lcms2.h`
//! by `rust/tools/gen_signatures.py`. Do not edit by hand; regenerate.
//!
//! C declares these as `typedef enum` / `#define`; here they are `u32`
//! constants in `pub mod`s named after each C enum.

#![allow(non_upper_case_globals)]
#![allow(missing_docs)]
#![allow(clippy::identity_op)]

use crate::types::Signature;

pub mod tagtype {
    use super::Signature;
    pub const SigChromaticityType: Signature = 0x6368726D; // 'chrm'
    pub const SigcicpType: Signature = 0x63696370; // 'cicp' 
    pub const SigColorantOrderType: Signature = 0x636C726F; // 'clro'
    pub const SigColorantTableType: Signature = 0x636C7274; // 'clrt'
    pub const SigCrdInfoType: Signature = 0x63726469; // 'crdi'
    pub const SigCurveType: Signature = 0x63757276; // 'curv'
    pub const SigDataType: Signature = 0x64617461; // 'data'
    pub const SigDictType: Signature = 0x64696374; // 'dict'
    pub const SigDateTimeType: Signature = 0x6474696D; // 'dtim'
    pub const SigDeviceSettingsType: Signature = 0x64657673; // 'devs'
    pub const SigLut16Type: Signature = 0x6d667432; // 'mft2'
    pub const SigLut8Type: Signature = 0x6d667431; // 'mft1'
    pub const SigLutAtoBType: Signature = 0x6d414220; // 'mAB '
    pub const SigLutBtoAType: Signature = 0x6d424120; // 'mBA '
    pub const SigMeasurementType: Signature = 0x6D656173; // 'meas'
    pub const SigMultiLocalizedUnicodeType: Signature = 0x6D6C7563; // 'mluc'
    pub const SigMultiProcessElementType: Signature = 0x6D706574; // 'mpet'
    pub const SigNamedColorType: Signature = 0x6E636f6C; // 'ncol' -- DEPRECATED!
    pub const SigNamedColor2Type: Signature = 0x6E636C32; // 'ncl2'
    pub const SigParametricCurveType: Signature = 0x70617261; // 'para'
    pub const SigProfileSequenceDescType: Signature = 0x70736571; // 'pseq'
    pub const SigProfileSequenceIdType: Signature = 0x70736964; // 'psid'
    pub const SigResponseCurveSet16Type: Signature = 0x72637332; // 'rcs2'
    pub const SigS15Fixed16ArrayType: Signature = 0x73663332; // 'sf32'
    pub const SigScreeningType: Signature = 0x7363726E; // 'scrn'
    pub const SigSignatureType: Signature = 0x73696720; // 'sig '
    pub const SigTextType: Signature = 0x74657874; // 'text'
    pub const SigTextDescriptionType: Signature = 0x64657363; // 'desc'
    pub const SigU16Fixed16ArrayType: Signature = 0x75663332; // 'uf32'
    pub const SigUcrBgType: Signature = 0x62666420; // 'bfd '
    pub const SigUInt16ArrayType: Signature = 0x75693136; // 'ui16'
    pub const SigUInt32ArrayType: Signature = 0x75693332; // 'ui32'
    pub const SigUInt64ArrayType: Signature = 0x75693634; // 'ui64'
    pub const SigUInt8ArrayType: Signature = 0x75693038; // 'ui08'
    pub const SigVcgtType: Signature = 0x76636774; // 'vcgt'
    pub const SigViewingConditionsType: Signature = 0x76696577; // 'view'
    pub const SigXYZType: Signature = 0x58595A20; // 'XYZ '
    pub const SigMHC2Type: Signature = 0x4D484332; // 'MHC2'
}

pub mod tag {
    use super::Signature;
    pub const SigAToB0Tag: Signature = 0x41324230; // 'A2B0'
    pub const SigAToB1Tag: Signature = 0x41324231; // 'A2B1'
    pub const SigAToB2Tag: Signature = 0x41324232; // 'A2B2'
    pub const SigBlueColorantTag: Signature = 0x6258595A; // 'bXYZ'
    pub const SigBlueMatrixColumnTag: Signature = 0x6258595A; // 'bXYZ'
    pub const SigBlueTRCTag: Signature = 0x62545243; // 'bTRC'
    pub const SigBToA0Tag: Signature = 0x42324130; // 'B2A0'
    pub const SigBToA1Tag: Signature = 0x42324131; // 'B2A1'
    pub const SigBToA2Tag: Signature = 0x42324132; // 'B2A2'
    pub const SigCalibrationDateTimeTag: Signature = 0x63616C74; // 'calt'
    pub const SigCharTargetTag: Signature = 0x74617267; // 'targ'
    pub const SigChromaticAdaptationTag: Signature = 0x63686164; // 'chad'
    pub const SigChromaticityTag: Signature = 0x6368726D; // 'chrm'
    pub const SigColorantOrderTag: Signature = 0x636C726F; // 'clro'
    pub const SigColorantTableTag: Signature = 0x636C7274; // 'clrt'
    pub const SigColorantTableOutTag: Signature = 0x636C6F74; // 'clot'
    pub const SigColorimetricIntentImageStateTag: Signature = 0x63696973; // 'ciis'
    pub const SigCopyrightTag: Signature = 0x63707274; // 'cprt'
    pub const SigCrdInfoTag: Signature = 0x63726469; // 'crdi'
    pub const SigDataTag: Signature = 0x64617461; // 'data'
    pub const SigDateTimeTag: Signature = 0x6474696D; // 'dtim'
    pub const SigDeviceMfgDescTag: Signature = 0x646D6E64; // 'dmnd'
    pub const SigDeviceModelDescTag: Signature = 0x646D6464; // 'dmdd'
    pub const SigDeviceSettingsTag: Signature = 0x64657673; // 'devs'
    pub const SigDToB0Tag: Signature = 0x44324230; // 'D2B0'
    pub const SigDToB1Tag: Signature = 0x44324231; // 'D2B1'
    pub const SigDToB2Tag: Signature = 0x44324232; // 'D2B2'
    pub const SigDToB3Tag: Signature = 0x44324233; // 'D2B3'
    pub const SigBToD0Tag: Signature = 0x42324430; // 'B2D0'
    pub const SigBToD1Tag: Signature = 0x42324431; // 'B2D1'
    pub const SigBToD2Tag: Signature = 0x42324432; // 'B2D2'
    pub const SigBToD3Tag: Signature = 0x42324433; // 'B2D3'
    pub const SigGamutTag: Signature = 0x67616D74; // 'gamt'
    pub const SigGrayTRCTag: Signature = 0x6b545243; // 'kTRC'
    pub const SigGreenColorantTag: Signature = 0x6758595A; // 'gXYZ'
    pub const SigGreenMatrixColumnTag: Signature = 0x6758595A; // 'gXYZ'
    pub const SigGreenTRCTag: Signature = 0x67545243; // 'gTRC'
    pub const SigLuminanceTag: Signature = 0x6C756d69; // 'lumi'
    pub const SigMeasurementTag: Signature = 0x6D656173; // 'meas'
    pub const SigMediaBlackPointTag: Signature = 0x626B7074; // 'bkpt'
    pub const SigMediaWhitePointTag: Signature = 0x77747074; // 'wtpt'
    pub const SigNamedColorTag: Signature = 0x6E636f6C; // 'ncol' // Deprecated by the ICC
    pub const SigNamedColor2Tag: Signature = 0x6E636C32; // 'ncl2'
    pub const SigOutputResponseTag: Signature = 0x72657370; // 'resp'
    pub const SigPerceptualRenderingIntentGamutTag: Signature = 0x72696730; // 'rig0'
    pub const SigPreview0Tag: Signature = 0x70726530; // 'pre0'
    pub const SigPreview1Tag: Signature = 0x70726531; // 'pre1'
    pub const SigPreview2Tag: Signature = 0x70726532; // 'pre2'
    pub const SigProfileDescriptionTag: Signature = 0x64657363; // 'desc'
    pub const SigProfileDescriptionMLTag: Signature = 0x6473636d; // 'dscm'
    pub const SigProfileSequenceDescTag: Signature = 0x70736571; // 'pseq'
    pub const SigProfileSequenceIdTag: Signature = 0x70736964; // 'psid'
    pub const SigPs2CRD0Tag: Signature = 0x70736430; // 'psd0'
    pub const SigPs2CRD1Tag: Signature = 0x70736431; // 'psd1'
    pub const SigPs2CRD2Tag: Signature = 0x70736432; // 'psd2'
    pub const SigPs2CRD3Tag: Signature = 0x70736433; // 'psd3'
    pub const SigPs2CSATag: Signature = 0x70733273; // 'ps2s'
    pub const SigPs2RenderingIntentTag: Signature = 0x70733269; // 'ps2i'
    pub const SigRedColorantTag: Signature = 0x7258595A; // 'rXYZ'
    pub const SigRedMatrixColumnTag: Signature = 0x7258595A; // 'rXYZ'
    pub const SigRedTRCTag: Signature = 0x72545243; // 'rTRC'
    pub const SigSaturationRenderingIntentGamutTag: Signature = 0x72696732; // 'rig2'
    pub const SigScreeningDescTag: Signature = 0x73637264; // 'scrd'
    pub const SigScreeningTag: Signature = 0x7363726E; // 'scrn'
    pub const SigTechnologyTag: Signature = 0x74656368; // 'tech'
    pub const SigUcrBgTag: Signature = 0x62666420; // 'bfd '
    pub const SigViewingCondDescTag: Signature = 0x76756564; // 'vued'
    pub const SigViewingConditionsTag: Signature = 0x76696577; // 'view'
    pub const SigVcgtTag: Signature = 0x76636774; // 'vcgt'
    pub const SigMetaTag: Signature = 0x6D657461; // 'meta'
    pub const SigcicpTag: Signature = 0x63696370; // 'cicp'
    pub const SigArgyllArtsTag: Signature = 0x61727473; // 'arts'
    pub const SigMHC2Tag: Signature = 0x4D484332; // 'MHC2'
}

pub mod technology {
    use super::Signature;
    pub const SigDigitalCamera: Signature = 0x6463616D; // 'dcam'
    pub const SigFilmScanner: Signature = 0x6673636E; // 'fscn'
    pub const SigReflectiveScanner: Signature = 0x7273636E; // 'rscn'
    pub const SigInkJetPrinter: Signature = 0x696A6574; // 'ijet'
    pub const SigThermalWaxPrinter: Signature = 0x74776178; // 'twax'
    pub const SigElectrophotographicPrinter: Signature = 0x6570686F; // 'epho'
    pub const SigElectrostaticPrinter: Signature = 0x65737461; // 'esta'
    pub const SigDyeSublimationPrinter: Signature = 0x64737562; // 'dsub'
    pub const SigPhotographicPaperPrinter: Signature = 0x7270686F; // 'rpho'
    pub const SigFilmWriter: Signature = 0x6670726E; // 'fprn'
    pub const SigVideoMonitor: Signature = 0x7669646D; // 'vidm'
    pub const SigVideoCamera: Signature = 0x76696463; // 'vidc'
    pub const SigProjectionTelevision: Signature = 0x706A7476; // 'pjtv'
    pub const SigCRTDisplay: Signature = 0x43525420; // 'CRT '
    pub const SigPMDisplay: Signature = 0x504D4420; // 'PMD '
    pub const SigAMDisplay: Signature = 0x414D4420; // 'AMD '
    pub const SigPhotoCD: Signature = 0x4B504344; // 'KPCD'
    pub const SigPhotoImageSetter: Signature = 0x696D6773; // 'imgs'
    pub const SigGravure: Signature = 0x67726176; // 'grav'
    pub const SigOffsetLithography: Signature = 0x6F666673; // 'offs'
    pub const SigSilkscreen: Signature = 0x73696C6B; // 'silk'
    pub const SigFlexography: Signature = 0x666C6578; // 'flex'
    pub const SigMotionPictureFilmScanner: Signature = 0x6D706673; // 'mpfs'
    pub const SigMotionPictureFilmRecorder: Signature = 0x6D706672; // 'mpfr'
    pub const SigDigitalMotionPictureCamera: Signature = 0x646D7063; // 'dmpc'
    pub const SigDigitalCinemaProjector: Signature = 0x64636A70; // 'dcpj'
}

pub mod colorspace {
    use super::Signature;
    pub const SigXYZData: Signature = 0x58595A20; // 'XYZ '
    pub const SigLabData: Signature = 0x4C616220; // 'Lab '
    pub const SigLuvData: Signature = 0x4C757620; // 'Luv '
    pub const SigYCbCrData: Signature = 0x59436272; // 'YCbr'
    pub const SigYxyData: Signature = 0x59787920; // 'Yxy '
    pub const SigRgbData: Signature = 0x52474220; // 'RGB '
    pub const SigGrayData: Signature = 0x47524159; // 'GRAY'
    pub const SigHsvData: Signature = 0x48535620; // 'HSV '
    pub const SigHlsData: Signature = 0x484C5320; // 'HLS '
    pub const SigCmykData: Signature = 0x434D594B; // 'CMYK'
    pub const SigCmyData: Signature = 0x434D5920; // 'CMY '
    pub const SigMCH1Data: Signature = 0x4D434831; // 'MCH1'
    pub const SigMCH2Data: Signature = 0x4D434832; // 'MCH2'
    pub const SigMCH3Data: Signature = 0x4D434833; // 'MCH3'
    pub const SigMCH4Data: Signature = 0x4D434834; // 'MCH4'
    pub const SigMCH5Data: Signature = 0x4D434835; // 'MCH5'
    pub const SigMCH6Data: Signature = 0x4D434836; // 'MCH6'
    pub const SigMCH7Data: Signature = 0x4D434837; // 'MCH7'
    pub const SigMCH8Data: Signature = 0x4D434838; // 'MCH8'
    pub const SigMCH9Data: Signature = 0x4D434839; // 'MCH9'
    pub const SigMCHAData: Signature = 0x4D434841; // 'MCHA'
    pub const SigMCHBData: Signature = 0x4D434842; // 'MCHB'
    pub const SigMCHCData: Signature = 0x4D434843; // 'MCHC'
    pub const SigMCHDData: Signature = 0x4D434844; // 'MCHD'
    pub const SigMCHEData: Signature = 0x4D434845; // 'MCHE'
    pub const SigMCHFData: Signature = 0x4D434846; // 'MCHF'
    pub const SigNamedData: Signature = 0x6e6d636c; // 'nmcl'
    pub const Sig1colorData: Signature = 0x31434C52; // '1CLR'
    pub const Sig2colorData: Signature = 0x32434C52; // '2CLR'
    pub const Sig3colorData: Signature = 0x33434C52; // '3CLR'
    pub const Sig4colorData: Signature = 0x34434C52; // '4CLR'
    pub const Sig5colorData: Signature = 0x35434C52; // '5CLR'
    pub const Sig6colorData: Signature = 0x36434C52; // '6CLR'
    pub const Sig7colorData: Signature = 0x37434C52; // '7CLR'
    pub const Sig8colorData: Signature = 0x38434C52; // '8CLR'
    pub const Sig9colorData: Signature = 0x39434C52; // '9CLR'
    pub const Sig10colorData: Signature = 0x41434C52; // 'ACLR'
    pub const Sig11colorData: Signature = 0x42434C52; // 'BCLR'
    pub const Sig12colorData: Signature = 0x43434C52; // 'CCLR'
    pub const Sig13colorData: Signature = 0x44434C52; // 'DCLR'
    pub const Sig14colorData: Signature = 0x45434C52; // 'ECLR'
    pub const Sig15colorData: Signature = 0x46434C52; // 'FCLR'
    pub const SigLuvKData: Signature = 0x4C75764B; // 'LuvK'
}

pub mod profileclass {
    use super::Signature;
    pub const SigInputClass: Signature = 0x73636E72; // 'scnr'
    pub const SigDisplayClass: Signature = 0x6D6E7472; // 'mntr'
    pub const SigOutputClass: Signature = 0x70727472; // 'prtr'
    pub const SigLinkClass: Signature = 0x6C696E6B; // 'link'
    pub const SigAbstractClass: Signature = 0x61627374; // 'abst'
    pub const SigColorSpaceClass: Signature = 0x73706163; // 'spac'
    pub const SigNamedColorClass: Signature = 0x6e6d636c; // 'nmcl'
    // iccMAX only
    pub const SigColorEncodingSpaceClass: Signature = 0x63656E63; // 'cenc' 
    pub const SigMultiplexIdentificationClass: Signature = 0x6D696420; // 'mid '
    pub const SigMultiplexLinkClass: Signature = 0x6d6c6e6b; // 'mlnk'
    pub const SigMultiplexVisualizationClass: Signature = 0x6d766973; // 'mvis'    
}

pub mod platform {
    use super::Signature;
    pub const SigMacintosh: Signature = 0x4150504C; // 'APPL'
    pub const SigMicrosoft: Signature = 0x4D534654; // 'MSFT'
    pub const SigSolaris: Signature = 0x53554E57; // 'SUNW'
    pub const SigSGI: Signature = 0x53474920; // 'SGI '
    pub const SigTaligent: Signature = 0x54474E54; // 'TGNT'
    pub const SigUnices: Signature = 0x2A6E6978; // '*nix'   // From argyll -- Not official
}

pub mod stage {
    use super::Signature;
    pub const SigCurveSetElemType: Signature = 0x63767374; //'cvst'
    pub const SigMatrixElemType: Signature = 0x6D617466; //'matf'
    pub const SigCLutElemType: Signature = 0x636C7574; //'clut'
    pub const SigBAcsElemType: Signature = 0x62414353; // 'bACS'
    pub const SigEAcsElemType: Signature = 0x65414353; // 'eACS'
    // Custom from here, not in the ICC Spec
    pub const SigXYZ2LabElemType: Signature = 0x6C327820; // 'l2x '
    pub const SigLab2XYZElemType: Signature = 0x78326C20; // 'x2l '
    pub const SigNamedColorElemType: Signature = 0x6E636C20; // 'ncl '
    pub const SigLabV2toV4: Signature = 0x32203420; // '2 4 '
    pub const SigLabV4toV2: Signature = 0x34203220; // '4 2 '
    // Identities
    pub const SigIdentityElemType: Signature = 0x69646E20; // 'idn '
    // Float to floatPCS
    pub const SigLab2FloatPCS: Signature = 0x64326C20; // 'd2l '
    pub const SigFloatPCS2Lab: Signature = 0x6C326420; // 'l2d '
    pub const SigXYZ2FloatPCS: Signature = 0x64327820; // 'd2x '
    pub const SigFloatPCS2XYZ: Signature = 0x78326420; // 'x2d '  
    pub const SigClipNegativesElemType: Signature = 0x636c7020; // 'clp '
}

pub mod curveseg {
    use super::Signature;
    pub const SigFormulaCurveSeg: Signature = 0x70617266; // 'parf'
    pub const SigSampledCurveSeg: Signature = 0x73616D66; // 'samf'
    pub const SigSegmentedCurve: Signature = 0x63757266; // 'curf'
}

pub mod stageloc {
    use super::Signature;
    pub const AT_BEGIN: Signature = 0;
}

pub mod infotype {
    use super::Signature;
    pub const InfoDescription: Signature = 0;
    pub const InfoManufacturer: Signature = 1;
    pub const InfoModel: Signature = 2;
    pub const InfoCopyright: Signature = 3;
}

pub mod psresourcetype {
    use super::Signature;
    pub const PS_RESOURCE_CSA: Signature = 0;
}

pub mod misc {
    use super::Signature;
    pub const SigPerceptualReferenceMediumGamut: Signature = 0x70726d67; //'prmg'
    pub const SigSceneColorimetryEstimates: Signature = 0x73636F65; //'scoe'
    pub const SigSceneAppearanceEstimates: Signature = 0x73617065; //'sape'
    pub const SigFocalPlaneColorimetryEstimates: Signature = 0x66706365; //'fpce'
    pub const SigReflectionHardcopyOriginalColorimetry: Signature = 0x72686F63; //'rhoc'
    pub const SigReflectionPrintOutputColorimetry: Signature = 0x72706F63; //'rpoc'
    pub const SigStatusA: Signature = 0x53746141; //'StaA'
    pub const SigStatusE: Signature = 0x53746145; //'StaE'
    pub const SigStatusI: Signature = 0x53746149; //'StaI'
    pub const SigStatusT: Signature = 0x53746154; //'StaT'
    pub const SigStatusM: Signature = 0x5374614D; //'StaM'
    pub const SigDN: Signature = 0x444E2020; //'DN  '
    pub const SigDNP: Signature = 0x444E2050; //'DN P'
    pub const SigDNN: Signature = 0x444E4E20; //'DNN '
    pub const SigDNNP: Signature = 0x444E4E50; //'DNNP'
}
