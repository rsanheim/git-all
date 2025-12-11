# frozen_string_literal: true

require "spec_helper"

RSpec.describe Nit::Commands::Status do
  let(:formatter) { described_class.new }

  def output(stdout: "", stderr: "", success: true)
    Nit::Output.new(stdout:, stderr:, success:)
  end

  describe "#format" do
    context "when output is clean" do
      it 'returns "clean" for empty porcelain output' do
        result = formatter.format(output(stdout: ""))
        expect(result).to eq("clean")
      end
    end

    context "with modified files" do
      it "counts staged modifications" do
        result = formatter.format(output(stdout: "M  file.txt\n"))
        expect(result).to eq("1 modified")
      end

      it "counts unstaged modifications" do
        result = formatter.format(output(stdout: " M file.txt\n"))
        expect(result).to eq("1 modified")
      end

      it "counts both staged and unstaged" do
        porcelain = <<~OUT
          M  staged.txt
           M unstaged.txt
        OUT
        result = formatter.format(output(stdout: porcelain))
        expect(result).to eq("2 modified")
      end
    end

    context "with added files" do
      it "counts added files" do
        result = formatter.format(output(stdout: "A  new_file.txt\n"))
        expect(result).to eq("1 added")
      end
    end

    context "with deleted files" do
      it "counts staged deletions" do
        result = formatter.format(output(stdout: "D  removed.txt\n"))
        expect(result).to eq("1 deleted")
      end

      it "counts unstaged deletions" do
        result = formatter.format(output(stdout: " D removed.txt\n"))
        expect(result).to eq("1 deleted")
      end
    end

    context "with renamed files" do
      it "counts renamed files" do
        result = formatter.format(output(stdout: "R  old.txt -> new.txt\n"))
        expect(result).to eq("1 renamed")
      end
    end

    context "with untracked files" do
      it "counts untracked files" do
        result = formatter.format(output(stdout: "?? untracked.txt\n"))
        expect(result).to eq("1 untracked")
      end
    end

    context "with mixed changes" do
      it "formats multiple types in order" do
        porcelain = <<~OUT
          M  modified.txt
          A  added.txt
          D  deleted.txt
          R  renamed.txt
          ?? untracked.txt
        OUT
        result = formatter.format(output(stdout: porcelain))
        expect(result).to eq("1 modified, 1 added, 1 deleted, 1 renamed, 1 untracked")
      end
    end

    context "on error" do
      it "returns error message from stderr" do
        result = formatter.format(output(
          stderr: "fatal: not a git repository\n",
          success: false
        ))
        expect(result).to eq("ERROR: fatal: not a git repository")
      end

      it 'returns "unknown error" when stderr is empty' do
        result = formatter.format(output(stderr: "", success: false))
        expect(result).to eq("ERROR: unknown error")
      end
    end
  end

  describe "#build_command" do
    it "creates status command with --porcelain" do
      cmd = formatter.build_command("/path/to/repo")
      expect(cmd.to_s).to eq("git -C /path/to/repo status --porcelain")
    end

    it "appends extra args after --porcelain" do
      cmd = formatter.build_command("/path/to/repo", ["--ignored"])
      expect(cmd.to_s).to eq("git -C /path/to/repo status --porcelain --ignored")
    end
  end
end
