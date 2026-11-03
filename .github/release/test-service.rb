# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Run with brew ruby; exercise the generated formula with this release's binaries.
require "formula"
require "tmpdir"
require "fileutils"
require "open3"
require "timeout"

path = Pathname(ARGV.fetch(0)).expand_path
binaries = Pathname(ARGV.fetch(1)).expand_path
source = Pathname.pwd
load path
formula = Norn.new("norn", path, :stable, tap: Tap.fetch("shokki-engineering/tap"))

Dir.mktmpdir("norn-service-") do |temporary|
  root = Pathname(temporary)
  formula.define_singleton_method(:prefix) { @test_prefix }
  formula.define_singleton_method(:etc) { (root/"etc").extend(InstallRenamed) }
  formula.define_singleton_method(:var) { root/"var" }
  formula.define_singleton_method(:opt_prefix) { root/"opt/norn" }
  formula.buildpath = root/"build"
  formula.buildpath.mkpath
  install = lambda do |version|
    formula.instance_variable_set(:@test_prefix, root/"Cellar/norn"/version)
    %w[norn nornctl].each { |binary| FileUtils.cp(binaries/binary, formula.buildpath/binary) }
    (formula.buildpath/"docs").mkpath
    %w[LICENSE LICENSE-COMMERCIAL.md].each do |document|
      FileUtils.cp(source/document, formula.buildpath/document)
    end
    Dir.chdir(formula.buildpath) { formula.install }
    %w[LICENSE LICENSE-COMMERCIAL.md].each do |document|
      raise "missing or changed #{document}" unless (formula.prefix/document).read == (source/document).read
    end
    Homebrew::InstallSteps::Runner.new(context: formula).run(formula.post_install_steps)
  end
  install.call("0.1.0")
  [formula.var/"norn", formula.var/"run/norn"].each do |directory|
    raise "insecure directory" unless directory.stat.mode & 0777 == 0700
  end
  log = formula.var/"log/norn.log"
  raise "insecure log" unless log.stat.mode & 0777 == 0600
  config = formula.etc/"norn/norn.toml"
  endpoint = "unix:#{formula.var}/run/norn/norn.sock"
  raise "wrong socket" unless config.read.include?(endpoint)
  raise "MCP enabled by default" unless config.read.include?("enabled = false")
  edited = config.read + "\n# Preserve my configuration\n"
  config.write(edited)
  log.write("existing log\n")
  install.call("0.1.1-test")
  raise "configuration overwritten" unless config.read == edited
  raise "log overwritten" unless log.read == "existing log\n"
  formula.opt_prefix.dirname.mkpath
  FileUtils.ln_sf(formula.prefix, formula.opt_prefix)
  service = formula.service
  plist = service.to_plist
  unit = service.to_systemd_unit
  raise "missing login service" unless plist.include?("RunAtLoad") && plist.include?("norn.toml")
  raise "missing restart policy" unless service.keep_alive? && unit.include?("Restart=on-failure")
  raise "wrong working directory" unless service.working_dir == formula.var.join("norn").to_s
  raise "wrong service log" unless unit.include?("StandardError=append:#{log}")
  2.times do
    pid = Process.spawn(*service.run, chdir: service.working_dir, out: [log.to_s, "a"], err: [:child, :out])
    begin
      Timeout.timeout(15) do
        loop do
          output, _, status = Open3.capture3((formula.bin/"nornctl").to_s, "--endpoint", endpoint, "status")
          break if status.success? && output.include?("health: SERVING")

          sleep 0.05
        end
      end
    rescue StandardError
      warn log.read
      raise
    ensure
      Process.kill("TERM", pid)
      _, status = Timeout.timeout(20) { Process.wait2(pid) }
      raise "unclean SIGTERM" unless status.success?
    end
  end
  puts "Homebrew install, root licenses, permissions, preserved config/logs, login service, health and restart passed"
end
