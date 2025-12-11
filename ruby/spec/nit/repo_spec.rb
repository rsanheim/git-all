# frozen_string_literal: true

require "spec_helper"

RSpec.describe Nit::Repo do
  describe ".repo_name" do
    it "extracts the basename from a path" do
      expect(described_class.repo_name("/home/user/src/my-repo")).to eq("my-repo")
    end

    it "handles paths with trailing slash" do
      expect(described_class.repo_name("/home/user/src/my-repo/")).to eq("my-repo")
    end
  end

  describe ".format_repo_name" do
    it "pads short names to 24 characters" do
      result = described_class.format_repo_name("my-repo")
      expect(result).to eq("[my-repo                 ]")
      expect(result.length).to eq(26) # [ + 24 + ]
    end

    it "handles exact length names" do
      name = "exactly-twenty-four-chr"
      expect(name.length).to eq(23) # one less than max
      result = described_class.format_repo_name(name)
      expect(result.length).to eq(26)
    end

    it "truncates long names with -..." do
      result = described_class.format_repo_name("this-is-a-very-long-repository-name")
      expect(result).to eq("[this-is-a-very-long--...]")
      expect(result.length).to eq(26)
    end
  end

  describe ".find_git_repos" do
    it "finds git repos in a directory", :aggregate_failures do
      Dir.mktmpdir do |tmpdir|
        # Create some directories
        repo1 = File.join(tmpdir, "repo1")
        repo2 = File.join(tmpdir, "repo2")
        not_a_repo = File.join(tmpdir, "not-a-repo")

        FileUtils.mkdir_p(File.join(repo1, ".git"))
        FileUtils.mkdir_p(File.join(repo2, ".git"))
        FileUtils.mkdir_p(not_a_repo)

        repos = described_class.find_git_repos(tmpdir)

        expect(repos.length).to eq(2)
        expect(repos).to include(repo1)
        expect(repos).to include(repo2)
        expect(repos).not_to include(not_a_repo)
      end
    end

    it "returns repos sorted alphabetically" do
      Dir.mktmpdir do |tmpdir|
        %w[zebra alpha middle].each do |name|
          FileUtils.mkdir_p(File.join(tmpdir, name, ".git"))
        end

        repos = described_class.find_git_repos(tmpdir)
        names = repos.map { |r| File.basename(r) }

        expect(names).to eq(%w[alpha middle zebra])
      end
    end

    it "returns empty array when no repos found" do
      Dir.mktmpdir do |tmpdir|
        expect(described_class.find_git_repos(tmpdir)).to eq([])
      end
    end
  end
end
