# frozen_string_literal: true

require "spec_helper"

RSpec.describe Nit::GitCommand do
  describe "#to_s" do
    it "builds command string with repo path and command" do
      cmd = described_class.new("/path/to/repo", "status")
      expect(cmd.to_s).to eq("git -C /path/to/repo status")
    end

    it "includes args when present" do
      cmd = described_class.new("/path/to/repo", "pull", ["--all", "--verbose"])
      expect(cmd.to_s).to eq("git -C /path/to/repo pull --all --verbose")
    end

    it "handles empty args" do
      cmd = described_class.new("/path/to/repo", "fetch", [])
      expect(cmd.to_s).to eq("git -C /path/to/repo fetch")
    end
  end

  describe "#execute" do
    it "runs git command and captures output" do
      Dir.mktmpdir do |tmpdir|
        # Initialize a git repo
        system("git", "-C", tmpdir, "init", "--quiet")

        cmd = described_class.new(tmpdir, "status", ["--porcelain"])
        output = cmd.execute

        expect(output).to be_a(Nit::Output)
        expect(output.success).to be true
        expect(output.stderr).to eq("")
      end
    end

    it "captures stderr on error" do
      Dir.mktmpdir do |tmpdir|
        cmd = described_class.new(tmpdir, "status")
        output = cmd.execute

        expect(output.success).to be false
        expect(output.stderr).to include("not a git repository")
      end
    end
  end
end

RSpec.describe Nit::Runner do
  let(:runner) { described_class.new(workers: 2, dry_run: false) }

  describe "#initialize" do
    it "sets default workers to 8" do
      default_runner = described_class.new
      expect(default_runner.workers).to eq(8)
    end

    it "accepts custom worker count" do
      custom_runner = described_class.new(workers: 4)
      expect(custom_runner.workers).to eq(4)
    end

    it "defaults dry_run to false" do
      expect(runner.dry_run).to be false
    end
  end

  describe "#run_parallel" do
    let(:formatter) do
      Class.new do
        def format(output)
          output.success ? "ok" : "error"
        end
      end.new
    end

    it "runs commands across repos in parallel" do
      Dir.mktmpdir do |tmpdir|
        repo1 = File.join(tmpdir, "repo1")
        repo2 = File.join(tmpdir, "repo2")

        [repo1, repo2].each do |repo|
          FileUtils.mkdir_p(repo)
          system("git", "-C", repo, "init", "--quiet")
        end

        output_lines = []
        allow(runner).to receive(:puts) { |line| output_lines << line }

        build_command = ->(repo_path) { Nit::GitCommand.new(repo_path, "status", ["--porcelain"]) }
        runner.run_parallel([repo1, repo2], build_command:, formatter:)

        expect(output_lines.length).to eq(2)
        expect(output_lines).to all(include("ok"))
      end
    end

    it "outputs dry-run commands when dry_run is true" do
      dry_runner = described_class.new(workers: 1, dry_run: true)

      output_lines = []
      allow(dry_runner).to receive(:puts) { |line| output_lines << line }

      build_command = ->(repo_path) { Nit::GitCommand.new(repo_path, "pull") }
      dry_runner.run_parallel(["/fake/repo1", "/fake/repo2"], build_command:, formatter:)

      expect(output_lines).to contain_exactly(
        "git -C /fake/repo1 pull",
        "git -C /fake/repo2 pull"
      )
    end
  end
end

RSpec.describe Nit::CommandResult do
  describe "DryRun" do
    it "stores command string" do
      result = Nit::CommandResult::DryRun.new(command_string: "git status")
      expect(result.command_string).to eq("git status")
    end
  end

  describe "Executed" do
    it "stores repo name and output" do
      output = Nit::Output.new(stdout: "ok", stderr: "", success: true)
      result = Nit::CommandResult::Executed.new(repo_name: "my-repo", output:)

      expect(result.repo_name).to eq("my-repo")
      expect(result.output.success).to be true
    end
  end

  describe "Error" do
    it "stores repo name and message" do
      result = Nit::CommandResult::Error.new(repo_name: "my-repo", message: "failed")
      expect(result.repo_name).to eq("my-repo")
      expect(result.message).to eq("failed")
    end
  end
end
