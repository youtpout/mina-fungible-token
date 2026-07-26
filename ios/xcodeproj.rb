# Generates ios/MinaTokenTransfer.xcodeproj — the one thing a device build needs
# that swiftc cannot provide, since installing on an iPhone means signing and
# provisioning and those live in an Xcode target.
#
#   ios/xcodeproj.sh
#
# The project is generated, not committed: it carries no team id, and the source
# of truth is this script. Nothing here is specific to a machine, so the only
# thing to set after opening it is the signing team.
#
# Both the iOS SDKs are wired up — device and simulator link the archive for
# their own triple, built by the pre-build phase.

require 'fileutils'
require 'xcodeproj'

here = File.expand_path(__dir__)
project_path = File.join(here, 'MinaTokenTransfer.xcodeproj')
FileUtils.rm_rf(project_path)
project = Xcodeproj::Project.new(project_path)

target = project.new_target(:application, 'MinaTokenTransfer', :ios, '17.0')

# The Swift screen, shared with the macOS build verbatim, and the C interface as
# a bridging header. The generated defaults may not exist yet on a fresh clone —
# the pre-build phase writes them — so the reference is created either way.
group = project.new_group('Sources', 'Sources')
sources = %w[
  MinaTokenTransferApp.swift
  TransferView.swift
  FormButtonStyle.swift
  MinaBackend.swift
  Defaults.generated.swift
]
sources.each do |name|
  file = group.new_reference(name)
  target.add_file_references([file])
end

archive = 'shared/native/target/%<triple>s/release/libmina_token_mobile.a'
device_archive = format(archive, triple: 'aarch64-apple-ios')
simulator_archive = format(archive, triple: 'aarch64-apple-ios-sim')

# What the archive itself pulls in, as reported by
#
#   cargo rustc --release --lib --target aarch64-apple-ios -- --print native-static-libs
#
# which is the first thing to re-run if a dependency change turns into a link
# error. Xcode passes -lSystem -lc -lm itself, so they are left off here.
system_flags = '-framework Security -framework CoreFoundation -liconv'

settings = {
  'PRODUCT_NAME' => 'Mina token transfer',
  'PRODUCT_BUNDLE_IDENTIFIER' => 'com.lumina.minatokennative',
  'MARKETING_VERSION' => '0.1.0',
  'CURRENT_PROJECT_VERSION' => '1',
  'SWIFT_VERSION' => '5.0',
  'IPHONEOS_DEPLOYMENT_TARGET' => '17.0',
  'TARGETED_DEVICE_FAMILY' => '1,2',
  'CODE_SIGN_STYLE' => 'Automatic',
  'SWIFT_OBJC_BRIDGING_HEADER' => '$(SRCROOT)/include/mina.h',
  'GENERATE_INFOPLIST_FILE' => 'YES',
  'INFOPLIST_KEY_UILaunchScreen_Generation' => 'YES',
  'INFOPLIST_KEY_UIStatusBarStyle' => 'UIStatusBarStyleLightContent',
  'INFOPLIST_KEY_UISupportedInterfaceOrientations' =>
    'UIInterfaceOrientationPortrait UIInterfaceOrientationLandscapeLeft ' \
    'UIInterfaceOrientationLandscapeRight',
  # The pre-build phase shells out to cargo, which the script sandbox blocks.
  'ENABLE_USER_SCRIPT_SANDBOXING' => 'NO',
  # By path, not -l: cargo puts a .dylib of the same name beside the archive
  # (that is what Android loads) and the linker would prefer it, leaving the app
  # asking for a dynamic library that will not be on the phone.
  'OTHER_LDFLAGS[sdk=iphoneos*]' => "$(SRCROOT)/../#{device_archive} #{system_flags}",
  'OTHER_LDFLAGS[sdk=iphonesimulator*]' => "$(SRCROOT)/../#{simulator_archive} #{system_flags}",
}

target.build_configurations.each do |configuration|
  configuration.build_settings.merge!(settings)
  # A debug build of the prover is far too slow to be worth offering, so both
  # configurations link the release archive; only the Swift changes.
  configuration.build_settings['ONLY_ACTIVE_ARCH'] = 'YES' if configuration.name == 'Debug'
end

phase = target.new_shell_script_build_phase('Build the Rust prover')
phase.shell_script = '"$SRCROOT/build-rust.sh"'
phase.always_out_of_date = '1'
# Run it before the Swift compile, not after.
target.build_phases.unshift(target.build_phases.delete(phase))

project.save

# A shared scheme, so `xcodebuild -scheme` works and Xcode does not have to
# invent one on first open.
scheme = Xcodeproj::XCScheme.new
scheme.configure_with_targets(target, nil)
scheme.set_launch_target(target)
scheme.save_as(project_path, 'MinaTokenTransfer', true)

puts "wrote #{project_path}"
