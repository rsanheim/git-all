# frozen_string_literal: true

require "spec_helper"

RSpec.describe Nit::Commands::Fetch do
  let(:formatter) { described_class.new }

  def output(stdout: "", stderr: "", success: true)
    Nit::Output.new(stdout:, stderr:, success:)
  end

  describe "#format" do
    context "when nothing to fetch" do
      it 'returns "no new commits" for empty output' do
        result = formatter.format(output(stdout: "", stderr: ""))
        expect(result).to eq("no new commits")
      end

      it "ignores From lines in stderr" do
        result = formatter.format(output(stderr: "From github.com:user/repo\n"))
        expect(result).to eq("no new commits")
      end
    end

    context "with branch updates" do
      it "counts single branch update" do
        stdout = "   abc123..def456  main       -> origin/main\n"
        result = formatter.format(output(stdout:))
        expect(result).to eq("1 branch updated")
      end

      it "pluralizes multiple branches" do
        stdout = <<~OUT
             abc123..def456  main       -> origin/main
             111111..222222  develop    -> origin/develop
        OUT
        result = formatter.format(output(stdout:))
        expect(result).to eq("2 branches updated")
      end
    end

    context "with tag updates" do
      it "counts single tag" do
        stdout = " * [new tag]         v1.0.0     -> v1.0.0\n"
        result = formatter.format(output(stdout:))
        expect(result).to eq("1 tag updated")
      end

      it "pluralizes multiple tags" do
        stdout = <<~OUT
           * [new tag]         v1.0.0     -> v1.0.0
           * [new tag]         v1.1.0     -> v1.1.0
        OUT
        result = formatter.format(output(stdout:))
        expect(result).to eq("2 tags updated")
      end
    end

    context "with mixed updates" do
      it "counts branches and tags" do
        stdout = <<~OUT
             abc123..def456  main       -> origin/main
           * [new tag]         v1.0.0     -> v1.0.0
        OUT
        result = formatter.format(output(stdout:))
        expect(result).to eq("1 branch, 1 tag updated")
      end
    end

    context "on error" do
      it "returns error message from stderr" do
        result = formatter.format(output(
          stderr: "fatal: Could not read from remote\n",
          success: false
        ))
        expect(result).to eq("ERROR: fatal: Could not read from remote")
      end
    end
  end

  describe "#build_command" do
    it "creates fetch command" do
      cmd = formatter.build_command("/path/to/repo")
      expect(cmd.to_s).to eq("git -C /path/to/repo fetch")
    end

    it "includes extra args" do
      cmd = formatter.build_command("/path/to/repo", ["--all", "--prune"])
      expect(cmd.to_s).to eq("git -C /path/to/repo fetch --all --prune")
    end
  end
end
